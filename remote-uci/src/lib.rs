mod engine;
pub mod uci;
mod ws;

use std::{
    cmp::min,
    error::Error,
    fs, io,
    net::{SocketAddr, TcpListener},
    ops::Not,
    path::PathBuf,
    sync::Arc,
    thread,
};

use axum::{
    response::Redirect,
    routing::{get, IntoMakeService},
    Router,
};
use clap::Parser;
use engine::EngineParameters;
use hyper::server::conn::AddrIncoming;
use listenfd::ListenFd;
use serde::Serialize;
use serde_with::{serde_as, CommaSeparator, DisplayFromStr, StringWithSeparator};
use sysinfo::{RefreshKind, System, SystemExt};

use crate::{
    engine::Engine,
    ws::{Secret, SharedEngine},
};


/// External UCI engine provider for lichess.org.
#[derive(Debug, Parser)]
#[clap(version)]
pub struct Opts {
    #[clap(flatten)]
    engine: EngineOpts,
    /// Bind server on this socket address.
    #[clap(long)]
    bind: Option<SocketAddr>,
    /// The publically accessible address used when registering with lichess
    #[clap(long)]
    publish_addr: Option<String>,
    /// Pass this flag if the public_addr endpoint uses TLS
    #[clap(long)]
    publish_addr_tls: bool,
    /// Overwrite engine name.
    #[clap(long)]
    name: Option<String>,
    /// Limit number of threads.
    #[clap(long)]
    max_threads: Option<u32>,
    /// Limit size of hash table (MiB).
    #[clap(long)]
    max_hash: Option<u32>,
    /// Provide file with secret token to use instead of a random one.
    #[clap(long)]
    secret_file: Option<PathBuf>,
    /// Promise that the selected engine is a recent official Stockfish
    /// release.
    #[clap(long, hide = true)]
    promise_official_stockfish: bool,
}

#[derive(Debug, Parser)]
pub struct EngineOpts {
    /// UCI engine executable to use if the CPU supports the x86-64 feature
    /// VNNI512.
    #[clap(long, display_order = 0)]
    engine_x86_64_vnni512: Option<PathBuf>,
    /// Or else, the UCI engine executable to use if the CPU supports the
    /// x64-64 feature AVX512.
    #[clap(long, display_order = 1)]
    engine_x86_64_avx512: Option<PathBuf>,
    /// Or else, the UCI engine executable to use if the CPU supports the
    /// x86-64 feature BMI2 with fast PEXT/PDEP.
    #[clap(long, display_order = 2)]
    engine_x86_64_bmi2: Option<PathBuf>,
    /// Or else, the UCI engine executable to use if the CPU supports the
    /// x86-64 feature AVX2.
    #[clap(long, display_order = 3)]
    engine_x86_64_avx2: Option<PathBuf>,
    /// Or else, the UCI engine executable to use if the CPU supports the
    /// x86-64 features SSE41 and POPCNT.
    #[clap(long, display_order = 4)]
    engine_x86_64_sse41_popcnt: Option<PathBuf>,
    /// Or else, the UCI engine executable to use if the CPU supports the
    /// x86-64 feature SSSE3.
    #[clap(long, display_order = 5)]
    engine_x86_64_ssse3: Option<PathBuf>,
    /// Or else, the UCI engine executable to use if the CPU supports the
    /// x86-64 features SSE3 and POPCNT.
    #[clap(long, display_order = 6)]
    engine_x86_64_sse3_popcnt: Option<PathBuf>,
    /// Or else, the UCI engine executable to use.
    #[clap(long, display_order = 7)]
    engine: PathBuf,
}

impl EngineOpts {
    #[cfg(target_arch = "x86_64")]
    fn best(self) -> PathBuf {
        self.engine_x86_64_vnni512
            .filter(|_| {
                is_x86_feature_detected!("avx512dq")
                    && is_x86_feature_detected!("avx512vl")
                    && is_x86_feature_detected!("avx512vnni")
            })
            .or(self.engine_x86_64_avx512)
            .filter(|_| is_x86_feature_detected!("avx512f") && is_x86_feature_detected!("avx512bw"))
            .or(self.engine_x86_64_bmi2)
            .filter(|_| {
                is_x86_feature_detected!("bmi2") && {
                    // AMD was using slow software emulation for PEXT for a
                    // long time. The Zen 3 family (0x19) is the first to
                    // implement it in hardware.
                    let cpuid = raw_cpuid::CpuId::new();
                    cpuid
                        .get_vendor_info()
                        .map_or(true, |v| v.as_str() != "AuthenticAMD")
                        || cpuid
                            .get_feature_info()
                            .map_or(false, |f| f.family_id() >= 0x19)
                }
            })
            .or(self.engine_x86_64_avx2)
            .filter(|_| is_x86_feature_detected!("avx2"))
            .or(self.engine_x86_64_sse41_popcnt)
            .filter(|_| is_x86_feature_detected!("sse4.1"))
            .or(self.engine_x86_64_ssse3)
            .filter(|_| is_x86_feature_detected!("ssse3"))
            .or(self.engine_x86_64_sse3_popcnt)
            .filter(|_| is_x86_feature_detected!("sse3") && is_x86_feature_detected!("popcnt"))
            .unwrap_or(self.engine)
    }

    #[cfg(not(target_arch = "x86_64"))]
    fn best(self) -> PathBuf {
        self.engine
    }
}

#[serde_as]
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExternalWorkerOpts {
    url: String,
    secret: Secret,
    name: String,
    max_threads: i64,
    max_hash: i64,
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, String>")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    variants: Vec<String>,
    #[serde_as(as = "DisplayFromStr")]
    #[serde(skip_serializing_if = "Not::not")]
    official_stockfish: bool,
}

impl ExternalWorkerOpts {
    pub fn registration_url(&self) -> String {
        format!(
            "https://lichess.org/analysis/external?{}",
            serde_urlencoded::to_string(&self).expect("serialize spec"),
        )
    }
}

fn available_memory() -> u64 {
    let sys = System::new_with_specifics(RefreshKind::new().with_memory());
    (sys.available_memory() / 1024).next_power_of_two() / 2
}

fn get_external_protocol(tls: bool) -> String {
    match tls {
        true => "wss".to_string(),
        false => "ws".to_string(),
    }
}

pub async fn make_server(
    opts: Opts,
    mut listen_fds: ListenFd,
) -> Result<
    (
        ExternalWorkerOpts,
        hyper::Server<AddrIncoming, IntoMakeService<Router>>,
    ),
    Box<dyn Error>,
> {
    let secret = match opts.secret_file {
        Some(path) => match fs::read_to_string(&path) {
            Ok(secret) if secret.len() >= 8 => {
                log::debug!("Loaded secret file {path:?}");
                Secret(secret)
            }
            Ok(_) => {
                log::error!("Ignoring secret file {path:?} (too short)");
                Secret::random()
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                let secret = Secret::random();
                match fs::write(&path, &secret.0) {
                    Ok(()) => log::warn!("Created new secret file {path:?}"),
                    Err(err) => log::error!("Failed to create secret file {path:?}: {err}"),
                }
                secret
            }
            Err(err) => {
                log::error!("Failed to load secret file {path:?}: {err}");
                Secret::random()
            }
        },
        None => Secret::random(),
    };

    let listener = opts
        .bind
        .map(TcpListener::bind)
        .or_else(|| listen_fds.take_tcp_listener(0).transpose())
        .unwrap_or_else(|| TcpListener::bind("localhost:9670"))
        .map_err(|err| {
            log::error!("Could not bind server: {err}");
            err
        })?;

    let engine = Engine::new(
        opts.engine.best(),
        EngineParameters {
            max_threads: min(
                opts.max_threads.unwrap_or(u32::MAX),
                u32::try_from(usize::from(
                    thread::available_parallelism().expect("available threads"),
                ))
                .unwrap_or(u32::MAX),
            ),
            max_hash: min(
                opts.max_hash.unwrap_or(u32::MAX),
                u32::try_from(available_memory()).unwrap_or(u32::MAX),
            ),
        },
    )
    .await
    .map_err(|err| {
        log::error!("Could not start engine: {err}");
        err
    })?;
    
    let spec = ExternalWorkerOpts {
        url: format!(
                 "{}://{}/socket",
                 get_external_protocol(opts.publish_addr_tls),
                 opts.publish_addr.unwrap_or(listener.local_addr().expect("local addr").to_string())
        ),
        secret: secret.clone(),
        max_threads: engine.max_threads(),
        max_hash: engine.max_hash(),
        variants: engine.variants().to_vec(),
        name: engine.name().unwrap_or("remote-uci").to_owned(),
        official_stockfish: opts.promise_official_stockfish,
    };

    let engine = Arc::new(SharedEngine::new(engine));

    let app = Router::new()
        .route(
            "/",
            get({
                let spec = spec.clone();
                move || redirect(spec)
            }),
        )
        .route(
            "/socket",
            get({
                let engine = Arc::clone(&engine);
                let secret = secret;
                move |params, socket| ws::handler(engine, secret, params, socket)
            }),
        );

    Ok((
        spec,
        axum::Server::from_tcp(listener)?.serve(app.into_make_service()),
    ))
}

async fn redirect(spec: ExternalWorkerOpts) -> Redirect {
    Redirect::to(&spec.registration_url())
}
