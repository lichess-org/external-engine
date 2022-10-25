#!/usr/bin/env python

"""External engine provider example for lichess.org"""

import argparse
import logging
import requests
import sys
import os
import secrets
import subprocess
import multiprocessing
import contextlib
import time


def ok(res):
    try:
        res.raise_for_status()
    except requests.exceptions.HTTPError:
        logging.error("Response: %s", res.text)
        raise
    return res


def register_engine(args, http):
    res = ok(http.get(f"{args.lichess}/api/external-engine"))

    secret = args.provider_secret or secrets.token_urlsafe(32)

    registration = {
        "name": args.name,
        "maxThreads": args.max_threads,
        "maxHash": args.max_hash,
        "shallowDepth": args.shallow_depth,
        "deepDepth": args.deep_depth,
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
    engine = Engine(args)
    http = requests.Session()
    http.headers["Authorization"] = f"Bearer {args.token}"
    secret = register_engine(args, http)

    backoff = 1
    while True:
        try:
            res = ok(http.post(f"{args.broker}/api/external-engine/work", json={"providerSecret": secret}))
            if res.status_code != 200:
                if engine is not None and engine.idle_time() > args.keep_alive:
                    logging.info("Terminating idle engine")
                    engine.terminate()
                    engine = None
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
            logging.info("Handling job %s", job["id"])
            if engine is None:
                engine = Engine(args)
            with engine.analyse(job) as analysis_stream:
                ok(http.post(f"{args.broker}/api/external-engine/work/{job['id']}", data=analysis_stream))
        except requests.exceptions.ConnectionError:
            logging.info("Connection closed while streaming analysis")
        except requests.exceptions.RequestException as err:
            logging.exception("Error while submitting work")
            time.sleep(5)
        except EOFError:
            logging.exception("Engine died")
            engine = None
            time.sleep(5)


class Engine:
    def __init__(self, args):
        self.process = subprocess.Popen(args.engine, shell=True, stdin=subprocess.PIPE, stdout=subprocess.PIPE, bufsize=1, universal_newlines=True)
        self.args = args
        self.session_id = None
        self.hash = None
        self.threads = None
        self.multi_pv = None
        self.last_used = time.monotonic()

        self.uci()
        self.setoption("UCI_AnalyseMode", "true")
        self.setoption("UCI_Chess960", "true")

    def idle_time(self):
        return time.monotonic() - self.last_used

    def terminate(self):
        self.process.terminate()

    def send(self, command):
        logging.debug("%d << %s", self.process.pid, command)
        self.process.stdin.write(command + "\n")
        self.process.stdin.flush()

    def recv(self):
        while True:
            line = self.process.stdout.readline()
            if line == "":
                raise EOFError()

            line = line.rstrip()
            if not line:
                continue

            command_and_params = line.split(None, 1)

            if command_and_params[0] != "info":
                logging.debug("%d >> %s", self.process.pid, line)

            if len(command_and_params) == 1:
                return command_and_params[0], ""
            else:
                return command_and_params

    def uci(self):
        self.send("uci")
        while True:
            line, _ = self.recv()
            if line == "uciok":
                break

    def isready(self):
        self.send("isready")
        while True:
            line, _ = self.recv()
            if line == "readyok":
                break

    def setoption(self, name, value):
        self.send(f"setoption name {name} value {value}")

    @contextlib.contextmanager
    def analyse(self, job):
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
        if options_changed:
            self.isready()

        self.send(f"position fen {work['initialFen']} moves {' '.join(work['moves'])}")
        self.send(f"go depth {self.args.deep_depth if work['deep'] else self.args.shallow_depth}")

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
            self.send("stop")
            for _ in analysis:
                pass

        self.last_used = time.monotonic()


if __name__ == "__main__":
    logging.basicConfig(level=logging.DEBUG)

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--name", default="Alpha 2", help="Engine name to register")
    parser.add_argument("--engine", help="Shell command to launch UCI engine", required=True)
    parser.add_argument("--lichess", default="https://lichess.org", help="Defaults to https://lichess.org")
    parser.add_argument("--broker", default="https://engine.lichess.ovh", help="Defaults to https://engine.lichess.ovh")
    parser.add_argument("--token", default=os.environ.get("LICHESS_API_TOKEN"), help="API token with engine:read and engine:write scopes")
    parser.add_argument("--provider-secret", default=os.environ.get("PROVIDER_SECRET"), help="Optional fixed provider secret")
    parser.add_argument("--deep-depth", type=int, default=99)
    parser.add_argument("--shallow-depth", type=int, default=25)
    parser.add_argument("--max-threads", type=int, default=multiprocessing.cpu_count(), help="Maximum number of available threads")
    parser.add_argument("--max-hash", type=int, default=512, help="Maximum hash table size in MiB")
    parser.add_argument("--keep-alive", type=int, default=120, help="Number of seconds to keep an idle/unused engine process around")

    try:
        import argcomplete
    except ImportError:
        pass
    else:
        argcomplete.autocomplete(parser)

    args = parser.parse_args()

    if not args.token:
        print(f"Need LICHESS_API_TOKEN environment variable from {args.lichess}/account/oauth/token/create?scopes[]=engine:read&scopes[]=engine:write")
        sys.exit(128)

    main(args)
