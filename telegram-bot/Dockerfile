FROM python:3.12-slim

# Drop root before doing anything writable.
RUN groupadd --gid 1000 bot && \
    useradd --uid 1000 --gid bot --create-home --shell /bin/false bot

WORKDIR /app
RUN chown bot:bot /app

# Install deps as root (writing to /usr/local), then switch to the bot user.
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY --chown=bot:bot bot.py .

USER bot

CMD ["python", "bot.py"]
