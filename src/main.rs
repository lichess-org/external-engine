use std::{io, path::PathBuf, process::Stdio, sync::atomic::AtomicU64};

use clap::Parser;
use tokio::{
    process::{ChildStdin, ChildStdout, Command},
    sync::{mpsc, Mutex},
};

#[derive(Debug, Parser)]
struct Opt {
    engine: PathBuf,
}

fn secret() -> String {
    format!("{:032x}", rand::random::<u128>())
}

struct Engine {
    current: AtomicU64,
    pipes: Mutex<EnginePipes>,
}

struct EnginePipes {
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl Engine {
    async fn new(path: PathBuf) -> io::Result<Engine> {
        let mut process = Command::new(path)
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn()?;
        Ok(Engine {
            current: AtomicU64::new(0),
            pipes: Mutex::new(EnginePipes {
                stdin: process.stdin.take().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::BrokenPipe, "engine stdin closed")
                })?,
                stdout: process.stdout.take().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::BrokenPipe, "engine stdout closed")
                })?,
            }),
        })
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let opt = Opt::parse();
    let engine = Engine::new(opt.engine).await?;
    Ok(())
}
