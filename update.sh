#!/bin/bash

git pull

# Rebuild the container and start it
docker compose build && docker compose up -d
