
FROM docker.io/rust:alpine3.17 AS base
RUN apk update && apk add musl-dev openssl-dev openssl clang llvm pkgconfig gcc alpine-sdk git g++ perl make
RUN update-ca-certificates

# Create appuser
ENV USER=bobot
ENV UID=10001
ARG TARGETPLATFORM
#ENV CC_aarch64_unknown_linux_musl=clang
#ENV AR_aarch64_unknown_linux_musl=llvm-ar
#ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="-Clink-self-contained=yes -Clinker=rust-lld"


RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/home/bobot" \
    --shell "/sbin/nologin" \
    --uid "${UID}" \
    "${USER}"


WORKDIR /bobot

RUN rustup default stable

FROM base AS builder
COPY ./ .
ENV CC=gcc
ENV CXX=g++
RUN cargo install --no-default-features \
 --features runtime-async-std-rustls --features cli --features codegen \
 --features async-std  sea-orm-cli && \
cargo install --path .

FROM alpine:edge AS migrate
COPY --from=builder /etc/passwd /etc/passwd
COPY --from=builder /etc/group /etc/group
RUN apk add cargo openssl-dev
COPY --from=builder /usr/local/cargo/bin/sea-orm-cli /
RUN mkdir -p /migrate/migration/target && mkdir -p /home/bobot/.cargo/registry && \
chown -R bobot:bobot /home/bobot && chown -R bobot:bobot /migrate
USER bobot:bobot
ENV OPENSSL_NO_VENDOR=1
WORKDIR /migrate
VOLUME /migrate
COPY ./ ./

CMD [ "/sea-orm-cli", "migrate", "up" ]

FROM scratch AS prod

COPY --from=builder /etc/passwd /etc/passwd
COPY --from=builder /etc/group /etc/group
WORKDIR /bobot
COPY --from=builder /etc/ssl /etc/ssl
COPY --from=builder /usr/local/cargo/bin/dijkstra ./
USER bobot:bobot
VOLUME /config
ENTRYPOINT [ "/bobot/dijkstra", "--config", "/config/config.toml"]


FROM base AS dev
RUN rustup default stable && rustup component add rustfmt && \
 rustup toolchain install nightly && \	
 rustup component add rustfmt --toolchain nightly && \
 cargo install sea-orm-cli cargo-edit
RUN git clone https://github.com/rust-lang/rust-analyzer.git /opt/rust-analyzer && \
    cd /opt/rust-analyzer && \
   rustup override set nightly && \
   cargo xtask install --server && cargo clean
RUN git clone https://github.com/helix-editor/helix /opt/helix && \
    cd /opt/helix &&  rustup override set stable && \
     cargo install --locked --path helix-term && cargo clean

RUN apt update && apt install -y postgresql-client redis fish
RUN mkdir -p /bobot/target && chown -R bobot:bobot /bobot && \
chown -R bobot:bobot /usr/local && mkdir -p /bobot/migration/target && \
chown -R bobot:bobot /bobot/migration/target && mkdir -p /bobot/bot_impl/target && \
chown -R bobot:bobot /bobot
USER bobot:bobot
RUN mkdir -p /home/bobot/.config/helix && ln -sf /opt/helix/runtime /home/bobot/.config/helix/runtime
VOLUME /bobot
WORKDIR /bobot
RUN rustup default stable
ENV COLORTERM=truecolor
ENV TERM xterm-256color
COPY helix.toml /home/bobot/.config/helix/config.toml
CMD [ "/usr/bin/fish" ]
