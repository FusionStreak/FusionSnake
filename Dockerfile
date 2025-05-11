FROM rust:1.86-slim as builder

COPY . /usr/app
WORKDIR /usr/app

RUN cargo install --path .

CMD [ "fusion-snake" ]