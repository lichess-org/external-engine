FROM debian:bullseye-slim AS stockfish
RUN apt-get update && apt-get install -y xz-utils make
WORKDIR /stockfish
COPY stockfish .
RUN cd vendor && \
    sha256sum -c SHA256SUM && \
    tar xf sde-external-9.0.0-2021-11-07-lin.tar.xz && \
    tar xf x86_64-linux-musl-native.tgz && \
    mv nn-6877cd24400e.nnue Stockfish/src
ENV SDE_PATH /stockfish/vendor/sde-external-9.0.0-2021-11-07-lin/sde64
ENV CXX /stockfish/vendor/x86_64-linux-musl-native/bin/x86_64-linux-musl-g++
ENV STRIP /stockfish/vendor/x86_64-linux-musl-native/bin/strip
RUN mkdir -p usr/lib/stockfish && \
    cd vendor/Stockfish/src && \
    cp nn-*.nnue ../../../usr/lib/stockfish/ && \
    for arch in "x86-64-vnni512" "x86-64-avx512" "x86-64-bmi2" "x86-64-avx2" "x86-64-sse41-popcnt" "x86-64-ssse3" "x86-64-sse3-popcnt" "x86-64"; do \
        LDFLAGS=-static CXXFLAGS=-DNNUE_EMBEDDING_OFF make -B -j2 profile-build COMP=gcc CXX=${CXX} ARCH=${arch} EXE=stockfish-${arch} && \
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
CMD /usr/bin/remote-uci --bind 0.0.0.0:9670 stockfish
