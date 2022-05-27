#!/bin/sh -e
wget --no-clobber --input-file downloads.txt
docker build --tag stockfish .
image=$(docker create stockfish)
docker cp "$image:/stockfish_15-1_amd64.deb" .
