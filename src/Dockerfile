FROM rust:1-slim AS builder

COPY . /usr/app
WORKDIR /usr/app

RUN cargo install --path .

EXPOSE 6666

CMD [ "fusion-snake" ]
