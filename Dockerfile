
FROM docker.io/rust:alpine3.17 AS base
RUN apk update && apk add musl-dev openssl-dev openssl clang llvm pkgconfig gcc alpine-sdk git g++ perl make
RUN update-ca-certificates

# Create appuser
ENV USER=bobot
ENV UID=10001
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
RUN cargo install --target-dir ./target --path . && cargo install --target-dir ./target  --path ./migration/

FROM alpine:3.17 AS prod

COPY --from=builder /etc/passwd /etc/passwd
COPY --from=builder /etc/group /etc/group
WORKDIR /bobot
COPY --from=builder /etc/ssl /etc/ssl
COPY --from=builder /usr/local/cargo/bin/dijkstra ./
COPY --from=builder /usr/local/cargo/bin/dijkstra_migration ./
USER bobot:bobot
VOLUME /config
CMD [ "sh", "-c", "\
    /bobot/dijkstra_migration -u postgresql://$POSTGRES_USER:$(cat $POSTGRES_PASSWORD_FILE)@$POSTGRES_HOST/$POSTGRES_DB up && \
    /bobot/dijkstra --config /config/config.toml \
"]

FROM alpine:3.17 AS migrate

COPY --from=builder /etc/passwd /etc/passwd
COPY --from=builder /etc/group /etc/group
COPY --from=builder /etc/ssl /etc/ssl
COPY --from=builder /usr/local/cargo/bin/migration /
USER bobot:bobot
ENTRYPOINT [ "sh", "-c", "/migration -u postgresql://$POSTGRES_USER:$(cat $POSTGRES_PASSWORD_FILE)@$POSTGRES_HOST/$POSTGRES_DB up" ]

FROM rust:latest AS dev

ENV USER=bobot
ENV UID=10001

RUN apt update && apt install -y postgresql-client redis fish gdb lld libssl-dev valgrind
RUN rustup default stable && rustup component add rustfmt && \
 rustup toolchain install nightly && \
 rustup component add rustfmt --toolchain nightly && \
 cargo install sea-orm-cli cargo-edit
RUN git clone https://github.com/rust-lang/rust-analyzer.git /opt/rust-analyzer && \
    cd /opt/rust-analyzer && \
   rustup override set stable && \
   cargo xtask install --server && cargo clean
RUN git clone https://github.com/helix-editor/helix /opt/helix && \
    cd /opt/helix &&  rustup override set stable && \
     cargo install --locked --path helix-term && cargo clean

RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/home/bobot" \
    --shell "/sbin/nologin" \
    --uid "${UID}" \
    "${USER}"

RUN mkdir -p /bobot/target && chown -R bobot:bobot /bobot && \
chown -R bobot:bobot /usr/local && mkdir -p /bobot/migration/target && \
chown -R bobot:bobot /bobot/migration/target && mkdir -p /bobot/macros/target && \
chown -R bobot:bobot /bobot && \
chown -R bobot:bobot /home/bobot/. && \
chown -R bobot:bobot /opt/helix/runtime

USER bobot:bobot


RUN mkdir -p /home/bobot/.config/helix && ln -sf /opt/helix/runtime /home/bobot/.config/helix/runtime
VOLUME /bobot
WORKDIR /bobot
RUN rustup default stable
RUN cargo install bacon
ENV COLORTERM=truecolor
ENV TERM xterm-256color
COPY --chown=bobot:bobot helix.toml /home/bobot/.config/helix/config.toml
COPY --chown=bobot:bobot languages.toml /home/bobot/.config/helix/languages.toml
CMD [ "/usr/bin/fish" ]
