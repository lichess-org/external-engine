#!/bin/sh -e
./deps.sh
docker build . --tag external-engine
docker run -itP external-engine
