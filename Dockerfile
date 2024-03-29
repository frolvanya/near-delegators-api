FROM frolvlad/alpine-rust:latest as build

RUN apk update && apk upgrade
RUN apk add --no-cache openssl-dev build-base perl protoc

RUN cargo new --bin near-delegators-api

WORKDIR /near-delegators-api

COPY Cargo.toml Cargo.lock ./

RUN cargo build --release
RUN rm src/main.rs

COPY src src
RUN rm ./target/release/deps/near_delegators_api*
RUN cargo build --release

FROM alpine:latest 

RUN apk add --no-cache libgcc git openssh-client

EXPOSE 8000

COPY --from=build /near-delegators-api/target/release/near-delegators-api .

ENV RUST_LOG=info
ENV ROCKET_ADDRESS="0.0.0.0"
CMD ["./near-delegators-api"]

