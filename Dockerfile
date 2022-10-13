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

FROM debian:bullseye-slim AS linter
RUN apt-get update && apt-get install -y lintian
COPY --from=stockfish /stockfish_15-1_amd64.deb .
RUN lintian -I /stockfish_*_amd64.deb

FROM debian:bullseye-slim
COPY --from=stockfish /stockfish_15-1_amd64.deb .
RUN dpkg -i /stockfish_*_amd64.deb
ENTRYPOINT ["/usr/bin/stockfish"]
