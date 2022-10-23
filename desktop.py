#!/usr/bin/env python3

__author__ = "Niklas Fiekas"
__email__ = "niklas.fiekas@backscattering.de"
__version__ = "0.1.0"

from PyQt6.QtCore import *
from PyQt6.QtGui import *
from PyQt6.QtWidgets import *

import sys
import http.server

class OAuthRequestHandler(http.server.BaseHTTPRequestHandler):
    pass

class OAuthServer:
    def __init__(self):
        self.server = http.server.HTTPServer(("", 8080), OAuthRequestHandler)
        self.server.serve_forever()


class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()

        self.setWindowTitle("External engine")

if __name__ == "__main__":
    server = OAuthServer()
    sys.exit(0)

    app = QApplication(sys.argv)

    mainWindow = MainWindow()
    mainWindow.show()

    app.exec()
