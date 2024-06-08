FROM rust:1.78-alpine

COPY . /usr/app
WORKDIR /usr/app

RUN cargo install --path .

CMD ["fusion-snake"]
