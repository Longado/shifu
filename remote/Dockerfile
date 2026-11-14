FROM rust:1.99.1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY remote/Cargo.toml remote/Cargo.lock ./remote/
COPY remote/src ./remote/src
COPY remote/crypto/Cargo.toml ./remote/crypto/
COPY remote/crypto/src ./remote/crypto/src
RUN cargo build --locked --release --manifest-path remote/Cargo.toml

FROM debian:bookworm-slim
RUN useradd --uid 10001 --create-home shifu && mkdir /data && chown shifu /data
COPY --from=build /src/remote/target/release/shifu-remote /usr/local/bin/shifu-remote
COPY remote/web-dist /web
USER shifu
ENV SHIFU_REMOTE_HOME=/data/runtime
EXPOSE 8080
ENTRYPOINT ["shifu-remote"]
CMD ["relay", "--bind", "0.0.0.0", "--port", "8080", "--web-root", "/web", "--key-file", "/data/join-key"]
