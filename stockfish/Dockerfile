FROM debian:bullseye-slim AS builder

RUN apt-get update && apt-get install -y xz-utils make

COPY SHA256SUM .
COPY sde-external-9.0.0-2021-11-07-lin.tar.xz .
COPY x86_64-linux-musl-native.tgz .
COPY sf_15.tar.gz .
COPY nn-6877cd24400e.nnue .

RUN sha256sum --check SHA256SUM && \
    tar xf sde-external-9.0.0-2021-11-07-lin.tar.xz && \
    tar xf x86_64-linux-musl-native.tgz && \
    tar xf sf_15.tar.gz && \
    mv nn-6877cd24400e.nnue /Stockfish-sf_15/src

ENV SDE_PATH=/sde-external-9.0.0-2021-11-07-lin/sde64
ENV CXX=/x86_64-linux-musl-native/bin/x86_64-linux-musl-g++
ENV STRIP=/x86_64-linux-musl-native/bin/strip

RUN cd /Stockfish-sf_15/src; \
    for arch in "x86-64-vnni512" "x86-64-avx512" "x86-64-bmi2" "x86-64-avx2" "x86-64-sse41-popcnt" "x86-64-ssse3" "x86-64-sse3-popcnt" "x86-64"; do \
        LDFLAGS=-static CXXFLAGS=-DNNUE_EMBEDDING_OFF make -B profile-build COMP=gcc CXX=${CXX} ARCH=${arch} EXE=stockfish-${arch}; \
        ${STRIP} stockfish-${arch}; \
    done

COPY DEBIAN/control /stockfish_15-1_amd64/DEBIAN/
COPY usr/share/doc/stockfish/copyright //stockfish_15-1_amd64/usr/share/doc/stockfish/
COPY usr/bin/stockfish /stockfish_15-1_amd64/usr/bin/
RUN mkdir -p /stockfish_15-1_amd64/usr/lib/stockfish; \
    cp /Stockfish-sf_15/src/stockfish-* /stockfish_15-1_amd64/usr/lib/stockfish/; \
    cp /Stockfish-sf_15/src/nn-*.nnue /stockfish_15-1_amd64/usr/lib/stockfish/; \
    dpkg-deb --build /stockfish_15-1_amd64; \
    dpkg -i /stockfish_15-1_amd64.deb # test install

CMD ["/usr/bin/stockfish"]
