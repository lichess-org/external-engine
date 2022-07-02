FROM rust:1.62.0-alpine AS builder
WORKDIR /remote-uci
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y lintian
COPY DEBIAN/control /remote-uci_1-1_amd64/DEBIAN/
COPY LICENSE.txt /remote-uci_1-1_amd64/usr/share/doc/remote-uci/
COPY --from=builder /remote-uci/target/release/remote-uci /remote-uci_1-1_amd64/usr/bin/
RUN cd /remote-uci_1-1_amd64; \
    md5sum $(find * -type f -not -path 'DEBIAN/*') > DEBIAN/md5sums; \
    cd /; \
    dpkg-deb --build /remote-uci_1-1_amd64; \
    dpkg -i /remote-uci_1-1_amd64.deb; \
    lintian -I /remote-uci_1-1_amd64.deb
