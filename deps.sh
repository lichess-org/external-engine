#!/bin/sh -e
git submodule update --init --recursive || true
(cd stockfish/vendor && wget -nc -i downloads.txt)
