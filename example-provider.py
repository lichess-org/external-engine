#!/usr/bin/env python

"""External engine provider example for lichess.org"""

import argparse
import concurrent.futures
import contextlib
import logging
import multiprocessing
import os
import requests
import secrets
import subprocess
import sys
import time
import threading

_LOG_LEVEL_MAP = {
        "critical": logging.CRITICAL,
        "error": logging.CRITICAL,
        "warning": logging.WARNING,
        "info": logging.INFO,
        "debug": logging.DEBUG,
        "notset": logging.NOTSET,
        }


def ok(res):
    try:
        res.raise_for_status()
    except requests.exceptions.HTTPError:
        logging.error("Response: %s", res.text)
        raise
    return res


def register_engine(args, http, engine):
    res = ok(http.get(f"{args.lichess}/api/external-engine"))

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

    for engine in res.json():
        if engine["name"] == args.name:
            logging.info("Updating engine %s", engine["id"])
            ok(http.put(f"{args.lichess}/api/external-engine/{engine['id']}", json=registration))
            break
    else:
        logging.info("Registering new engine")
        ok(http.post(f"{args.lichess}/api/external-engine", json=registration))

    return secret


def main(args):
    executor = concurrent.futures.ThreadPoolExecutor(max_workers=1)
    engine = Engine(args)
    http = requests.Session()
    http.headers["Authorization"] = f"Bearer {args.token}"
    secret = register_engine(args, http, engine)

    last_future = concurrent.futures.Future()
    last_future.set_result(None)

    backoff = 1
    while True:
        try:
            res = ok(http.post(f"{args.broker}/api/external-engine/work", json={"providerSecret": secret}, timeout=12))
            if res.status_code != 200:
                if engine.alive and engine.idle_time() > args.keep_alive:
                    logging.info("Terminating idle engine")
                    engine.terminate()
                continue
            job = res.json()
        except requests.exceptions.RequestException as err:
            logging.error("Error while trying to acquire work: %s", err)
            backoff = min(backoff * 1.5, 10)
            time.sleep(backoff)
            continue
        else:
            backoff = 1

        try:
            engine.stop()
        except EOFError:
            pass
        last_future.result()

        if not engine.alive:
            engine = Engine(args)

        job_started = threading.Event()
        last_future = executor.submit(handle_job, args, engine, job, job_started)
        job_started.wait()


def handle_job(args, engine, job, job_started):
    try:
        logging.info("Handling job %s", job["id"])
        with engine.analyse(job, job_started) as analysis_stream:
            ok(requests.post(f"{args.broker}/api/external-engine/work/{job['id']}", data=analysis_stream))
    except requests.exceptions.ConnectionError:
        logging.info("Connection closed while streaming analysis")
    except requests.exceptions.RequestException as err:
        logging.exception("Error while submitting work")
        time.sleep(5)
    except EOFError:
        logging.exception("Engine died")
        time.sleep(5)
    finally:
        job_started.set()


class Engine:
    def __init__(self, args):
        self.process = subprocess.Popen(args.engine, shell=True, stdin=subprocess.PIPE, stdout=subprocess.PIPE, bufsize=1, universal_newlines=True)
        self.args = args
        self.session_id = None
        self.hash = None
        self.threads = None
        self.multi_pv = None
        self.uci_variant = None
        self.supported_variants = []
        self.last_used = time.monotonic()
        self.alive = True
        self.stop_lock = threading.Lock()

        self.uci()
        self.setoption("UCI_AnalyseMode", "true")
        self.setoption("UCI_Chess960", "true")
        for name, value in args.setoption:
            self.setoption(name, value)

    def idle_time(self):
        return time.monotonic() - self.last_used

    def terminate(self):
        self.process.terminate()
        self.alive = False

    def send(self, command):
        logging.debug("%d << %s", self.process.pid, command)
        self.process.stdin.write(command + "\n")
        self.process.stdin.flush()

    def recv(self):
        while True:
            line = self.process.stdout.readline()
            if line == "":
                self.alive = False
                raise EOFError()

            line = line.rstrip()
            if not line:
                continue

            logging.debug("%d >> %s", self.process.pid, line)

            command_and_params = line.split(None, 1)

            if len(command_and_params) == 1:
                return command_and_params[0], ""
            else:
                return command_and_params

    def uci(self):
        self.send("uci")
        while True:
            command, args = self.recv()
            if command == "option":
                name = None
                args = args.split()
                while args:
                    arg = args.pop(0)
                    if arg == "name":
                        name = args.pop(0)
                    elif name == "UCI_Variant" and arg == "var":
                        self.supported_variants.append(args.pop(0))
            elif command == "uciok":
                break

        if self.supported_variants:
            logging.info("Supported variants: %s", ", ".join(self.supported_variants))

    def isready(self):
        self.send("isready")
        while True:
            line, _ = self.recv()
            if line == "readyok":
                break

    def setoption(self, name, value):
        self.send(f"setoption name {name} value {value}")

    @contextlib.contextmanager
    def analyse(self, job, job_started):
        work = job["work"]

        if work["sessionId"] != self.session_id:
            self.session_id = work["sessionId"]
            self.send("ucinewgame")
            self.isready()

        options_changed = False
        if self.threads != work["threads"]:
            self.setoption("Threads", work["threads"])
            self.threads = work["threads"]
            options_changed = True
        if self.hash != work["hash"]:
            self.setoption("Hash", work["hash"])
            self.hash = work["hash"]
            options_changed = True
        if self.multi_pv != work["multiPv"]:
            self.setoption("MultiPV", work["multiPv"])
            self.multi_pv = work["multiPv"]
            options_changed = True
        if self.uci_variant != work["variant"]:
            self.setoption("UCI_Variant", work["variant"])
            self.uci_variant = work["variant"]
            options_changed = True
        if options_changed:
            self.isready()

        self.send(f"position fen {work['initialFen']} moves {' '.join(work['moves'])}")

        for key in ["movetime", "depth", "nodes"]:
            if key in work:
                self.send(f"go {key} {work[key]}")
                break

        job_started.set()

        def stream():
            while True:
                command, params = self.recv()
                if command == "bestmove":
                    break
                elif command == "info":
                    if "score" in params:
                        yield (command + " " + params + "\n").encode("utf-8")
                else:
                    logging.warning("Unexpected engine command: %s", command)

        analysis = stream()
        try:
            yield analysis
        finally:
            self.stop()
            for _ in analysis:
                pass

        self.last_used = time.monotonic()

    def stop(self):
        if self.alive:
            with self.stop_lock:
                self.send("stop")


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

    main(args)
