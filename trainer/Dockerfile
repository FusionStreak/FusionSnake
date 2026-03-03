FROM python:3.12-slim

# curl is used for the snake readiness probe in entrypoint.sh
RUN apt-get update && \
    apt-get install -y --no-install-recommends curl && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Install Python dependencies first (layer cache)
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Copy application code
COPY . .
RUN chmod +x entrypoint.sh

# Flask trigger server
EXPOSE 5050

CMD ["./entrypoint.sh"]
