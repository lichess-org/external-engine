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
        logging.exception("Response: %s", res.text)
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
    engine.uci()
    engine.setoption("UCI_AnalyseMode", "true")

    http = requests.Session()
    http.headers["Authorization"] = f"Bearer {args.token}"
    secret = register_engine(args, http)

    backoff = 1
    while True:
        try:
            res = ok(http.post(f"{args.broker}/api/external-engine/work", json={"providerSecret": secret}))
            if res.status_code != 200:
                continue
            job = res.json()
        except requests.exceptions.RequestException as err:
            logging.error("Error while trying to acquire work: %s", err)
            time.sleep(backoff)
            backoff = min(backoff * 1.5, 10)
            continue
        else:
            backoff = 1

        try:
            logging.info("Handling job %s", job["id"])
            with engine.analyse(job) as analysis_stream:
                ok(http.post(f"{args.broker}/api/external-engine/work/{job['id']}", data=analysis_stream))
        except requests.exceptions.ConnectionError:
            logging.info("Connection closed while streaming analysis")


class Engine:
    def __init__(self, args):
        self.process = subprocess.Popen(args.engine, shell=True, stdin=subprocess.PIPE, stdout=subprocess.PIPE, bufsize=1, universal_newlines=True)
        self.session = None
        self.hash = None
        self.threads = None

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
            logging.debug("%d >> %s", self.process.pid, line)
            if line:
                return line

    def recv_uci(self):
        command_and_args = self.recv().split(None, 1)
        if len(command_and_args) == 1:
            return command_and_args[0], ""
        else:
            return command_and_args

    def uci(self):
        self.send("uci")
        while True:
            line, _ = self.recv_uci()
            if line == "uciok":
                break

    def isready(self):
        self.send("isready")
        while True:
            line, _ = self.recv_uci()
            if line == "readyok":
                break

    def setoption(self, name, value):
        self.send(f"setoption name {name} value {value}")

    @contextlib.contextmanager
    def analyse(self, job):
        def stream():
            work = job["work"]

            if work["sessionId"] != self.session:
                self.session = work["sessionId"]
                self.send("ucinewgame")
                self.isready()

            if self.threads != work["threads"]:
                self.setoption("Threads", work["threads"])
                self.threads = work["threads"]
            if self.hash != work["hash"]:
                self.setoption("Hash", work["hash"])
                self.hash = work["hash"]
            self.setoption("MultiPV", work["multiPv"])
            self.isready()

            self.send(f"position fen {work['initialFen']} moves {' '.join(work['moves'])}")
            self.send(f"go depth {args.deep_depth if work['deep'] else args.shallow_depth}")

            while True:
                line = self.recv()

                if line.startswith("bestmove"):
                    break

                if "score" in line:
                    yield (line + "\n").encode("utf-8")

        analysis = stream()
        try:
            yield analysis
        finally:
            self.send("stop")
            for _ in analysis:
                pass


if __name__ == "__main__":
    logging.basicConfig(level=logging.DEBUG)

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--name", default="Example", help="Engine name to register")
    parser.add_argument("--engine", help="Shell command to launch UCI engine", required=True)
    parser.add_argument("--lichess", default="https://lichess.org", help="Defaults to https://lichess.org")
    parser.add_argument("--broker", default="https://engine.lichess.ovh", help="Defaults to https://engine.lichess.ovh")
    parser.add_argument("--token", default=os.environ.get("LICHESS_API_TOKEN"), help="API token with engine:read and engine:write scopes")
    parser.add_argument("--provider-secret", default=os.environ.get("PROVIDER_SECRET"), help="Optional fixed provider secret")
    parser.add_argument("--deep-depth", default=99)
    parser.add_argument("--shallow-depth", default=25)
    parser.add_argument("--max-threads", default=multiprocessing.cpu_count(), help="Maximum number of available threads")
    parser.add_argument("--max-hash", default=512, help="Maximum hash table size in MiB")

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
