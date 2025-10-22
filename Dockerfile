FROM rust:1.90-slim AS builder

COPY . /usr/app
WORKDIR /usr/app

RUN cargo install --path .

CMD [ "fusion-snake" ]