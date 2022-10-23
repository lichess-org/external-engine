#!/usr/bin/env python3

__author__ = "Niklas Fiekas"
__email__ = "niklas.fiekas@backscattering.de"
__version__ = "0.1.0"

from PyQt6.QtCore import *
from PyQt6.QtGui import *
from PyQt6.QtWidgets import *

import secrets
import sys
import hashlib
import http.server
import urllib.parse
import base64

def code_challenge(code_verifier):
    h = hashlib.sha256()
    h.update(code_verifier.encode("ascii"))
    return base64.urlsafe_b64encode(h.digest())

class OAuthRequestHandler(http.server.BaseHTTPRequestHandler):
    pass

class OAuthServer:
    def __init__(self):
        self.code_verifier = secrets.token_urlsafe(32)
        self.state = secrets.token_urlsafe(32)

        self.server = http.server.HTTPServer(("127.0.0.1", 0), OAuthRequestHandler)
        ip, port = self.server.server_address

    def url(self):
        params = urllib.parse.urlencode({
            "response_type": "code",
            "client_id": "com.github.lichess_org.external_engine",
            "code_challenge_method": "S256",
            "code_challenge": code_challenge(self.code_verifier),
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
    print(server.url())
    sys.exit(0)

    app = QApplication(sys.argv)

    mainWindow = MainWindow()
    mainWindow.show()

    app.exec()
