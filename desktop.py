#!/usr/bin/env python3

__author__ = "Niklas Fiekas"
__email__ = "niklas.fiekas@backscattering.de"
__version__ = "0.1.0"

from PyQt6.QtCore import *
from PyQt6.QtGui import *
from PyQt6.QtWidgets import *

import concurrent.futures
import secrets
import sys
import hashlib
import http.server
import urllib.parse
import base64
import threading

def code_challenge(code_verifier):
    h = hashlib.sha256()
    h.update(code_verifier.encode("ascii"))
    return base64.urlsafe_b64encode(h.digest())

class OAuthRequestHandler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        url = urllib.parse.urlparse(self.path)
        query = urllib.parse.parse_qs(url.query)

        try:
            code = query["code"][0]
            state = query["state"][0]
        except KeyError:
            self.answer(400, "Did not receive authorization code and state")
            return

        if state != self.server.state:
            self.answer(403, "Mismatching sate")
            return

        self.answer(200, "Nearly done ...")
        self.server.access_token.cancel()

    def answer(self, status, text):
        self.send_response(status)
        self.end_headers()
        self.wfile.write(text.encode("utf-8"))

class OAuthServer(http.server.HTTPServer):
    def __init__(self):
        self.code_verifier = secrets.token_urlsafe(32)
        self.state = secrets.token_urlsafe(32)
        self.access_token = concurrent.futures.Future()
        self.access_token.add_done_callback(self.access_token_callback)
        super().__init__(("127.0.0.1", 0), OAuthRequestHandler)
        threading.Thread(target=self.serve_forever, name="OAuthServer::serve_forever").start()

    def access_token_callback(self, future):
        threading.Thread(target=self.shutdown, name="OAuthServer::shutdown").start()

    def authorization_url(self):
        ip, port = self.server_address
        params = urllib.parse.urlencode({
            "response_type": "code",
            "client_id": "com.github.lichess_org.external_engine",
            "code_challenge_method": "S256",
            "code_challenge": code_challenge(self.code_verifier),
            "redirect_uri": f"http://{ip}:{port}/",
            "scope": "engine:read engine:write",
            "state": self.state,
        })
        return f"https://lichess.org/oauth?{params}"


class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()

        self.setWindowTitle("External engine")

if __name__ == "__main__":
    server = OAuthServer()
    print(server.authorization_url())
    sys.exit(0)

    app = QApplication(sys.argv)

    mainWindow = MainWindow()
    mainWindow.show()

    app.exec()
