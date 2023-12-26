FROM frolvlad/alpine-rust:latest as build

RUN apk update && apk upgrade
RUN apk add --no-cache openssl-dev build-base perl protoc

RUN cargo new --bin near-stake-delegators-scan

WORKDIR /near-stake-delegators-scan

COPY Cargo.toml Cargo.lock ./

RUN cargo build --release
RUN rm src/main.rs

COPY src src
RUN rm ./target/release/deps/near_stake_delegators_scan*
RUN cargo build --release

FROM --platform=linux/amd64 alpine:latest 

RUN apk add --no-cache libgcc git openssh-client

EXPOSE 8000

COPY . .
COPY --from=build /near-stake-delegators-scan/target/release/near-stake-delegators-scan .

ENV RUST_LOG=info
ENV ROCKET_ADDRESS="0.0.0.0"
CMD ["./near-stake-delegators-scan"]

