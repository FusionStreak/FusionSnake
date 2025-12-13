FROM rust:1.92-slim AS builder

COPY . /usr/app
WORKDIR /usr/app

RUN cargo install --path .

CMD [ "fusion-snake" ]
