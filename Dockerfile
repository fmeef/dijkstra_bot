
FROM docker.io/library/rust:latest AS builder
RUN apt update && apt install -y musl-tools musl-dev libssl-dev pkg-config musl-tools clang llvm 
RUN update-ca-certificates

# Create appuser
ENV USER=bobot
ENV UID=10001
ARG TARGETPLATFORM
ENV CC_aarch64_unknown_linux_musl=clang
ENV AR_aarch64_unknown_linux_musl=llvm-ar
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="-Clink-self-contained=yes -Clinker=rust-lld"


RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/nonexistent" \
    --shell "/sbin/nologin" \
    --no-create-home \
    --uid "${UID}" \
    "${USER}"


WORKDIR /bobot

COPY ./ .
RUN if [ "$TARGETPLATFORM" = "linux/amd64" ]; then ARCHITECTURE=x86_64; \
elif [ "$TARGETPLATFORM" = "linux/arm/v7" ]; then ARCHITECTURE=arm; \
elif [ "$TARGETPLATFORM" = "linux/arm64" ]; then ARCHITECTURE=aarch64; \
else ARCHITECTURE=x86_64; fi && \
rustup target add $ARCHITECTURE-unknown-linux-musl && \
#cargo install --target $ARCHITECTURE-unknown-linux-musl sea-orm-cli && \
 cargo install --target  $ARCHITECTURE-unknown-linux-musl --path .

FROM builder AS admin 
RUN apt update && apt install -y coreutils postgresql
RUN cargo install sea-orm-cli
CMD [ "tail -f" ]

FROM scratch

COPY --from=builder /etc/passwd /etc/passwd
COPY --from=builder /etc/group /etc/group
WORKDIR /bobot
COPY --from=builder /etc/ssl /etc/ssl
COPY --from=builder /usr/local/cargo/bin/bobot ./
USER bobot:bobot
VOLUME /config
ENTRYPOINT [ "/bobot/bobot", "--config", "/config/config.toml"]