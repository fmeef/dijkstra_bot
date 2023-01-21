
FROM docker.io/library/rust:latest AS base
RUN apt update && apt install -y musl-tools musl-dev libssl-dev pkg-config musl-tools clang llvm 
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

RUN --mount=type=cache,target=/bobot/target \
--mount=type=cache,target=/usr/local/rustup \
--mount=type=cache,target=/usr/local/cargo/registry \
if  [ "$TARGETPLATFORM" = "linux/amd64" ]; then ARCHITECTURE=x86_64; \
elif [ "$TARGETPLATFORM" = "linux/arm/v7" ]; then ARCHITECTURE=arm; \
elif [ "$TARGETPLATFORM" = "linux/arm64" ]; then ARCHITECTURE=aarch64; \
else ARCHITECTURE=x86_64; fi && \
rustup default stable && \
rustup target add $ARCHITECTURE-unknown-linux-musl 

FROM base AS builder
COPY ./ .
ENV CC=musl-gcc
RUN --mount=type=cache,target=/bobot/target \
--mount=type=cache,target=/usr/local/rustup \
--mount=type=cache,target=/usr/local/cargo/registry \
if  [ "$TARGETPLATFORM" = "linux/amd64" ]; then ARCHITECTURE=x86_64; \
elif [ "$TARGETPLATFORM" = "linux/arm/v7" ]; then ARCHITECTURE=arm; \
elif [ "$TARGETPLATFORM" = "linux/arm64" ]; then ARCHITECTURE=aarch64; \
else ARCHITECTURE=x86_64; fi && \
cargo install --target aarch64-unknown-linux-musl --no-default-features \
 --features runtime-async-std-rustls --features cli --features codegen \
 --features async-std  sea-orm-cli && \
cargo install --target  $ARCHITECTURE-unknown-linux-musl --path .

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
COPY --from=builder /usr/local/cargo/bin/bobot ./
COPY --from=builder /usr/local/cargo/bin/sea-orm-cli ./
USER bobot:bobot
VOLUME /config
ENTRYPOINT [ "/bobot/bobot", "--config", "/config/config.toml"]


FROM base AS dev
RUN --mount=type=cache,target=/bobot/target \
--mount=type=cache,target=/usr/local/rustup \
--mount=type=cache,target=/usr/local/cargo/registry \
rustup default stable && rustup component add rustfmt && \
 cargo install sea-orm-cli
RUN --mount=type=cache,target=/bobot/target \
--mount=type=cache,target=/usr/local/rustup \
--mount=type=cache,target=/usr/local/cargo/registry \
  git clone --depth 1 https://github.com/rust-lang/rust-analyzer.git /opt/rust-analyzer && \
    cd /opt/rust-analyzer && \
   cargo xtask install --server && cargo clean
RUN --mount=type=cache,target=/bobot/target \
--mount=type=cache,target=/usr/local/rustup \
--mount=type=cache,target=/usr/local/cargo/registry \
 git clone https://github.com/helix-editor/helix /opt/helix && \
    cd /opt/helix && rustup override set nightly && \
    cargo install --path helix-term && cargo clean

RUN apt update && apt install -y postgresql-client redis
RUN mkdir -p /bobot/target && chown -R bobot:bobot /bobot && \
chown -R bobot:bobot /usr/local && mkdir -p /bobot/migration/target && \
chown -R bobot:bobot /bobot/migration/target && mkdir -p /bobot/bobot_impl/target && \
chown -R bobot:bobot /bobot
USER bobot:bobot
RUN mkdir -p /home/bobot/.config/helix && ln -sf /opt/helix/runtime /home/bobot/.config/helix/runtime
VOLUME /bobot
WORKDIR /bobot
RUN rustup default stable
