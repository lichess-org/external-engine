#!/usr/bin/env python

import argparse
import asyncio
import logging
import multiprocessing
import os
import secrets
import sys
import time

import aiohttp


_LOG_LEVEL_MAP = {
    "critical": logging.CRITICAL,
    "error": logging.CRITICAL,
    "warning": logging.WARNING,
    "info": logging.INFO,
    "debug": logging.DEBUG,
    "notset": logging.NOTSET,
}


async def check_response(res: aiohttp.ClientResponse) -> aiohttp.ClientResponse:
    if res.status >= 400:
        logging.error("Response: %s", await res.text())
        res.raise_for_status()
    return res


async def register_engine(args, http: aiohttp.ClientSession, engine: "Engine") -> str:
    async with http.get(f"{args.lichess}/api/external-engine") as res:
        await check_response(res)
        engines = await res.json()

    secret = args.provider_secret or secrets.token_urlsafe(32)

    variants = {
        "chess",
        "antichess",
        "atomic",
        "crazyhouse",
        "horde",
        "kingofthehill",
        "racingkings",
        "3check",
    }

    registration = {
        "name": args.name,
        "maxThreads": args.max_threads,
        "maxHash": args.max_hash,
        "variants": [variant for variant in engine.supported_variants or ["chess"] if variant in variants],
        "providerSecret": secret,
    }

    for registered_engine in engines:
        if registered_engine["name"] == args.name:
            logging.info("Updating engine %s", registered_engine["id"])
            async with http.put(
                f"{args.lichess}/api/external-engine/{registered_engine['id']}",
                json=registration,
            ) as res:
                await check_response(res)
            break
    else:
        logging.info("Registering new engine")
        async with http.post(f"{args.lichess}/api/external-engine", json=registration) as res:
            await check_response(res)

    return secret


async def main(args) -> None:
    engine = await Engine.create(args)

    auth_headers = {"Authorization": f"Bearer {args.token}"}
    acquire_timeout = aiohttp.ClientTimeout(total=12)
    stream_timeout = aiohttp.ClientTimeout(total=None)

    async with (
        aiohttp.ClientSession(headers=auth_headers) as http,
        aiohttp.ClientSession(timeout=stream_timeout) as submit_http,
    ):
        secret = await register_engine(args, http, engine)

        last_job: asyncio.Task | None = None
        backoff = 1.0

        while True:
            try:
                async with http.post(
                    f"{args.broker}/api/external-engine/work",
                    json={"providerSecret": secret},
                    timeout=acquire_timeout,
                ) as res:
                    await check_response(res)
                    if res.status != 200:
                        if engine.alive and engine.idle_time() > args.keep_alive:
                            await engine.terminate()
                        continue
                    job = await res.json()
            except (aiohttp.ClientError, asyncio.TimeoutError) as err:
                logging.error("Error while trying to acquire work: %s", err)
                backoff = min(backoff * 1.5, 10)
                await asyncio.sleep(backoff)
                continue
            else:
                backoff = 1.0

            try:
                await engine.stop()
            except EOFError:
                pass

            if last_job is not None:
                await last_job

            if not engine.alive:
                engine = await Engine.create(args)

            job_started = asyncio.Event()
            last_job = asyncio.create_task(handle_job(args, submit_http, engine, job, job_started))
            await job_started.wait()


async def handle_job(
    args,
    http: aiohttp.ClientSession,
    engine: "Engine",
    job,
    job_started: asyncio.Event,
) -> None:
    try:
        logging.info("Handling job %s", job["id"])
        async with http.post(
            f"{args.broker}/api/external-engine/work/{job['id']}",
            data=engine.analyse(job, job_started),
        ) as res:
            await check_response(res)
    except aiohttp.ClientConnectionError:
        logging.info("Connection closed while streaming analysis")
    except aiohttp.ClientError:
        logging.exception("Error while submitting work")
        await asyncio.sleep(5)
    except EOFError:
        logging.exception("Engine died")
        await asyncio.sleep(5)
    finally:
        job_started.set()


class Engine:
    def __init__(self, args, process: asyncio.subprocess.Process):
        self.process = process
        self.args = args
        self.session_id = None
        self.hash = None
        self.threads = None
        self.multi_pv = None
        self.uci_variant = None
        self.supported_variants: list[str] = []
        self.last_used = time.monotonic()
        self.stop_lock = asyncio.Lock()

    @classmethod
    async def create(cls, args) -> "Engine":
        process = await asyncio.create_subprocess_shell(
            args.engine,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
        )
        self = cls(args, process)

        await self.uci()
        await self.setoption("UCI_AnalyseMode", "true")
        await self.setoption("UCI_Chess960", "true")
        for name, value in args.setoption:
            await self.setoption(name, value)

        return self

    def alive(self) -> bool:
        return self.process.returncode is None

    def idle_time(self) -> float:
        return time.monotonic() - self.last_used

    async def terminate(self) -> None:
        if not self.alive():
            return

        self.process.terminate()
        try:
            await asyncio.wait_for(self.process.wait(), timeout=2)
        except asyncio.TimeoutError:
            self.process.kill()
            await self.process.wait()

    async def send(self, command: str) -> None:
        if not self.alive():
            raise EOFError()

        assert self.process.stdin is not None
        logging.debug("%d << %s", self.process.pid, command)
        try:
            self.process.stdin.write((command + "\n").encode())
            await self.process.stdin.drain()
        except (BrokenPipeError, ConnectionResetError) as err:
            raise EOFError() from err

    async def recv(self) -> tuple[str, str]:
        assert self.process.stdout is not None

        while True:
            line = await self.process.stdout.readline()
            if not line:
                raise EOFError()

            text = line.decode("utf-8", errors="replace").rstrip()
            if not text:
                continue

            logging.debug("%d >> %s", self.process.pid, text)
            command_and_params = text.split(None, 1)

            if len(command_and_params) == 1:
                return command_and_params[0], ""
            return command_and_params[0], command_and_params[1]

    async def uci(self) -> None:
        await self.send("uci")
        while True:
            command, params = await self.recv()
            if command == "option":
                name = None
                params_parts = params.split()
                while params_parts:
                    arg = params_parts.pop(0)
                    if arg == "name" and params_parts:
                        name = params_parts.pop(0)
                    elif name == "UCI_Variant" and arg == "var" and params_parts:
                        self.supported_variants.append(params_parts.pop(0))
            elif command == "uciok":
                break

        if self.supported_variants:
            logging.info("Supported variants: %s", ", ".join(self.supported_variants))

    async def isready(self) -> None:
        await self.send("isready")
        while True:
            command, _ = await self.recv()
            if command == "readyok":
                break

    async def setoption(self, name, value) -> None:
        await self.send(f"setoption name {name} value {value}")

    async def analyse(self, job, job_started: asyncio.Event):
        work = job["work"]
        read_task: asyncio.Task | None = None

        try:
            if work["sessionId"] != self.session_id:
                self.session_id = work["sessionId"]
                await self.send("ucinewgame")
                await self.isready()

            options_changed = False
            if self.threads != work["threads"]:
                await self.setoption("Threads", work["threads"])
                self.threads = work["threads"]
                options_changed = True
            if self.hash != work["hash"]:
                await self.setoption("Hash", work["hash"])
                self.hash = work["hash"]
                options_changed = True
            if self.multi_pv != work["multiPv"]:
                await self.setoption("MultiPV", work["multiPv"])
                self.multi_pv = work["multiPv"]
                options_changed = True
            if self.uci_variant != work["variant"]:
                await self.setoption("UCI_Variant", work["variant"])
                self.uci_variant = work["variant"]
                options_changed = True
            if options_changed:
                await self.isready()

            await self.send(f"position fen {work['initialFen']} moves {' '.join(work['moves'])}")

            for key in ("movetime", "depth", "nodes"):
                if key in work:
                    await self.send(f"go {key} {work[key]}")
                    break

            job_started.set()
            last_ping = time.monotonic()

            read_task = asyncio.create_task(self.recv())

            while True:
                done, _ = await asyncio.wait({read_task}, timeout=max(0, 15 + last_ping - time.monotonic()))
                self.last_used = time.monotonic()
                if not done:
                    # To support long searches, a provider must send '{"keepalive":true}\n' in the
                    # same streamed work submission every 15 seconds.

                    yield b'{"keepalive":true}\n'
                    last_ping = time.monotonic()
                    continue

                command, params = read_task.result()
                read_task = None

                if command == "bestmove":
                    return

                read_task = asyncio.create_task(self.recv())

                if command == "info":
                    if "score" in params:
                        yield f"{command} {params}\n".encode()
                else:
                    logging.warning("Unexpected engine command: %s", command)
        finally:
            if read_task is not None and self.alive():
                try:
                    await self.stop()
                    await self.drain_to_bestmove(read_task)
                except EOFError:
                    pass

            self.last_used = time.monotonic()

    async def drain_to_bestmove(self, read_task: asyncio.Task) -> None:
        command, _ = await read_task
        while command != "bestmove" and self.alive():
            command, _ = await self.recv()

    async def stop(self) -> None:
        async with self.stop_lock:
            if self.alive():
                await self.send("stop")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__, fromfile_prefix_chars='@')
    parser.add_argument("--name", default="Alpha 2", help="Engine name to register")
    parser.add_argument("--engine", help="Shell command to launch UCI engine", required=True)
    parser.add_argument("--setoption", nargs=2, action="append", default=[], metavar=("NAME", "VALUE"), help="Set a custom UCI option")
    parser.add_argument("--lichess", default="https://lichess.org", help="Defaults to https://lichess.org")
    parser.add_argument("--broker", default="https://engine.lichess.ovh", help="Defaults to https://engine.lichess.ovh")
    parser.add_argument("--token", default=os.environ.get("LICHESS_API_TOKEN"), help="API token with engine:read and engine:write scopes")
    parser.add_argument("--provider-secret", default=os.environ.get("PROVIDER_SECRET"), help="Optional fixed provider secret")
    parser.add_argument("--max-threads", type=int, default=multiprocessing.cpu_count(), help="Maximum number of available threads")
    parser.add_argument("--max-hash", type=int, default=512, help="Maximum hash table size in MiB")
    parser.add_argument("--keep-alive", type=int, default=300, help="Number of seconds to keep an idle/unused engine process around")
    parser.add_argument("--log-level", default="info", choices=_LOG_LEVEL_MAP.keys(), help="Logging verbosity")

    try:
        import argcomplete
    except ImportError:
        pass
    else:
        argcomplete.autocomplete(parser)

    args = parser.parse_args()

    logging.basicConfig(level=_LOG_LEVEL_MAP[args.log_level])

    if not args.token:
        print(f"Need LICHESS_API_TOKEN environment variable from {args.lichess}/account/oauth/token/create?scopes[]=engine:read&scopes[]=engine:write")
        sys.exit(128)

    asyncio.run(main(args))
