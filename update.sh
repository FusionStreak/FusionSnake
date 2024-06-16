#!/bin/bash

# Rebuild the container
docker compose build

# Stop and remove the existing containers
docker compose down

# Launch the containers
docker compose up -d