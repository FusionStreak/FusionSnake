FROM python:3.12-slim

# Install cron + curl (for health checks)
RUN apt-get update && \
    apt-get install -y --no-install-recommends cron curl && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Install Python dependencies first (layer cache)
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Copy application code
COPY . .
RUN chmod +x entrypoint.sh

# Create log file for cron
RUN touch /var/log/trainer.log

CMD ["./entrypoint.sh"]
