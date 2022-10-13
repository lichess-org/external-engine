FROM niklasf/fishnet-builder:3 AS builder
ARG BUILD_THREADS=2
RUN apk --no-cache add dpkg
WORKDIR /stockfish
COPY stockfish .
RUN mkdir -p usr/lib/stockfish && \
    cd vendor/Stockfish/src && \
    make net && \
    cp nn-*.nnue ../../../usr/lib/stockfish/ && \
    for arch in "x86-64-vnni512" "x86-64-avx512" "x86-64-bmi2" "x86-64-avx2" "x86-64-sse41-popcnt" "x86-64-ssse3" "x86-64-sse3-popcnt" "x86-64"; do \
        CXXFLAGS=-DNNUE_EMBEDDING_OFF make -B "-j${BUILD_THREADS}" profile-build ARCH=${arch} EXE=stockfish-${arch} && \
        ${STRIP} stockfish-${arch} && \
        cp stockfish-${arch} ../../../usr/lib/stockfish/; \
    done

WORKDIR /stockfish_15-1_amd64
RUN cp -R /stockfish/DEBIAN /stockfish/usr . && \
    md5sum $(find * -type f -not -path 'DEBIAN/*') > DEBIAN/md5sums && \
    cat DEBIAN/md5sums && \
    cd / && \
    dpkg-deb --build stockfish_*

FROM rust:1.62.0-slim AS remote-uci
RUN rustup target add x86_64-unknown-linux-musl
WORKDIR /remote-uci
COPY remote-uci .
RUN cargo build --release --target x86_64-unknown-linux-musl
WORKDIR /remote-uci_1-1_amd64
RUN mkdir -p usr/bin && \
    cp -R /remote-uci/DEBIAN /remote-uci/usr . && \
    cp /remote-uci/target/x86_64-unknown-linux-musl/release/remote-uci usr/bin/ && \
    md5sum $(find * -type f -not -path 'DEBIAN/*') > DEBIAN/md5sums && \
    cat DEBIAN/md5sums && \
    cd / && \
    dpkg-deb --build remote-uci_*

FROM debian:bullseye-slim AS linter
RUN apt-get update && apt-get install -y lintian
COPY --from=stockfish /stockfish_15-1_amd64.deb .
RUN lintian -I /stockfish_*_amd64.deb
COPY --from=remote-uci /remote-uci_1-1_amd64.deb .
RUN lintian -I /remote-uci_*_amd64.deb

FROM debian:bullseye-slim
RUN apt-get update && apt-get install -y openssl
COPY --from=stockfish /stockfish_15-1_amd64.deb .
RUN dpkg -i /stockfish_*_amd64.deb
COPY --from=remote-uci /remote-uci_1-1_amd64.deb .
RUN dpkg -i /remote-uci_*_amd64.deb
EXPOSE 9670/tcp
ENV REMOTE_UCI_LOG info
ENTRYPOINT [ "/usr/bin/remote-uci", "--bind", "0.0.0.0:9670", "--engine", "stockfish"]
