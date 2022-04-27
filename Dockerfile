FROM registry.access.redhat.com/ubi8/ubi:latest as builder

RUN dnf -y install openssl openssl-devel gcc gcc-c++ make

ENV RUSTUP_HOME=/opt/rust
ENV CARGO_HOME=/opt/rust

RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal

ENV PATH "$PATH:$CARGO_HOME/bin"

RUN mkdir -p /usr/src/drogue-opcua-agent
ADD . /usr/src/drogue-opcua-agent

WORKDIR /usr/src/drogue-opcua-agent

RUN cargo build --release

FROM registry.access.redhat.com/ubi8/ubi-minimal:latest

LABEL org.opencontainers.image.source="https://github.com/drogue-iot/drogue-opcua-agent-rust"

COPY --from=builder /usr/src/drogue-opcua-agent/target/release/drogue-opcua-agent /

ENTRYPOINT [ "/drogue-opcua-agent" ]
