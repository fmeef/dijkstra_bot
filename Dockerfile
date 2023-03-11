
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

RUN if  [ "$TARGETPLATFORM" = "linux/amd64" ]; then ARCHITECTURE=x86_64; \
elif [ "$TARGETPLATFORM" = "linux/arm/v7" ]; then ARCHITECTURE=arm; \
elif [ "$TARGETPLATFORM" = "linux/arm64" ]; then ARCHITECTURE=aarch64; \
else ARCHITECTURE=x86_64; fi && \
rustup default stable && \
rustup target add $ARCHITECTURE-unknown-linux-musl 

FROM base AS builder
COPY ./ .
ENV CC=musl-gcc
RUN if  [ "$TARGETPLATFORM" = "linux/amd64" ]; then ARCHITECTURE=x86_64; \
elif [ "$TARGETPLATFORM" = "linux/arm/v7" ]; then ARCHITECTURE=arm; \
elif [ "$TARGETPLATFORM" = "linux/arm64" ]; then ARCHITECTURE=aarch64; \
else ARCHITECTURE=x86_64; fi && \
cargo install --target $ARCHITECTURE-unknown-linux-musl --no-default-features \
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
RUN rustup default stable && rustup component add rustfmt && \
 cargo install sea-orm-cli
RUN git clone https://github.com/rust-lang/rust-analyzer.git /opt/rust-analyzer && \
    cd /opt/rust-analyzer && \
    git checkout 2023-03-06 && \
   rustup override set nightly && \
   cargo xtask install --server && cargo clean
RUN git clone https://github.com/helix-editor/helix /opt/helix && \
    cd /opt/helix && git checkout 22.12 && rustup override set nightly && \
     cargo install --locked --path helix-term && cargo clean

RUN apt update && apt install -y postgresql-client redis fish
RUN mkdir -p /bobot/target && chown -R bobot:bobot /bobot && \
chown -R bobot:bobot /usr/local && mkdir -p /bobot/migration/target && \
chown -R bobot:bobot /bobot/migration/target && mkdir -p /bobot/bobot_impl/target && \
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