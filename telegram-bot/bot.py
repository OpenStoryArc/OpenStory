"""Telegram <-> OpenClaw bridge bot.

Receives messages from Telegram, forwards them to the OpenClaw gateway,
and streams the agent's responses back to the Telegram chat.

Environment variables:
    TELEGRAM_BOT_TOKEN       - Bot token from BotFather (required)
    TELEGRAM_ALLOWED_USER_ID - Telegram user ID allowed to use the bot (required)
    OPENCLAW_GATEWAY_URL     - OpenClaw gateway URL (default: http://openclaw:18789)
    OPENCLAW_AGENT_ID        - Agent ID to message (default: main)
"""

import asyncio
import json
import logging
import os
import sys

import websockets
from telegram import Update
from telegram.ext import (
    Application,
    CommandHandler,
    MessageHandler,
    ContextTypes,
    filters,
)

logging.basicConfig(
    format="%(asctime)s [%(name)s] %(levelname)s: %(message)s",
    level=logging.INFO,
)
logger = logging.getLogger("openclaw-telegram")

TELEGRAM_BOT_TOKEN = os.environ.get("TELEGRAM_BOT_TOKEN", "")
ALLOWED_USER_ID = int(os.environ.get("TELEGRAM_ALLOWED_USER_ID", "0"))
GATEWAY_URL = os.environ.get("OPENCLAW_GATEWAY_URL", "http://openclaw:18789")
AGENT_ID = os.environ.get("OPENCLAW_AGENT_ID", "main")

TELEGRAM_MAX_MESSAGE_LENGTH = 4096


def check_config():
    if not TELEGRAM_BOT_TOKEN:
        logger.error("TELEGRAM_BOT_TOKEN is required")
        sys.exit(1)
    if ALLOWED_USER_ID == 0:
        logger.error("TELEGRAM_ALLOWED_USER_ID is required")
        sys.exit(1)


def is_authorized(user_id: int) -> bool:
    return user_id == ALLOWED_USER_ID


def chunk_message(text: str) -> list[str]:
    """Split text into chunks that fit Telegram's message length limit."""
    if len(text) <= TELEGRAM_MAX_MESSAGE_LENGTH:
        return [text]

    chunks = []
    while text:
        if len(text) <= TELEGRAM_MAX_MESSAGE_LENGTH:
            chunks.append(text)
            break
        # Try to split at a newline near the limit
        split_at = text.rfind("\n", 0, TELEGRAM_MAX_MESSAGE_LENGTH)
        if split_at == -1 or split_at < TELEGRAM_MAX_MESSAGE_LENGTH // 2:
            split_at = TELEGRAM_MAX_MESSAGE_LENGTH
        chunks.append(text[:split_at])
        text = text[split_at:].lstrip("\n")
    return chunks


async def send_to_openclaw(message: str) -> str:
    """Send a message to the OpenClaw gateway and collect the response.

    Connects to the OpenClaw gateway WebSocket API, sends the user's message,
    and collects assistant text responses until the turn completes.

    NOTE: The WebSocket protocol here matches OpenClaw's gateway API.
    If the message format changes, update this function.
    """
    ws_url = GATEWAY_URL.replace("http://", "ws://").replace("https://", "wss://")
    ws_endpoint = f"{ws_url}/agent/{AGENT_ID}/ws"

    response_parts = []

    try:
        async with websockets.connect(ws_endpoint, open_timeout=30) as ws:
            # Send the user's message
            await ws.send(json.dumps({
                "type": "message",
                "content": message,
            }))

            # Collect response chunks until the turn completes
            async for raw in ws:
                try:
                    event = json.loads(raw)
                except json.JSONDecodeError:
                    continue

                event_type = event.get("type", "")

                if event_type == "assistant_message":
                    text = event.get("content", "")
                    if text:
                        response_parts.append(text)
                elif event_type == "text":
                    text = event.get("content", event.get("text", ""))
                    if text:
                        response_parts.append(text)
                elif event_type == "tool_use":
                    name = event.get("name", event.get("tool", "unknown"))
                    response_parts.append(f"[Using tool: {name}]")
                elif event_type == "tool_result":
                    pass  # Tool results are internal; don't echo them
                elif event_type in ("done", "turn_complete", "end"):
                    break
                elif event_type == "error":
                    error_msg = event.get("message", event.get("error", "Unknown error"))
                    response_parts.append(f"[Error: {error_msg}]")
                    break

    except websockets.exceptions.ConnectionClosed as e:
        if not response_parts:
            return f"[Connection closed: {e}]"
    except (OSError, asyncio.TimeoutError) as e:
        return f"[Could not reach OpenClaw gateway: {e}]"

    return "\n".join(response_parts) if response_parts else "[No response from agent]"


async def start_command(update: Update, context: ContextTypes.DEFAULT_TYPE) -> None:
    if not is_authorized(update.effective_user.id):
        await update.message.reply_text("Unauthorized.")
        return

    await update.message.reply_text(
        "OpenClaw agent bridge ready. Send me a message and I'll forward it to the agent.\n\n"
        f"Gateway: {GATEWAY_URL}\n"
        f"Agent: {AGENT_ID}"
    )


async def handle_message(update: Update, context: ContextTypes.DEFAULT_TYPE) -> None:
    user = update.effective_user
    if not is_authorized(user.id):
        logger.warning(f"Unauthorized message from user {user.id} ({user.username})")
        return

    user_text = update.message.text
    if not user_text:
        return

    logger.info(f"Forwarding message to OpenClaw: {user_text[:100]}...")

    # Send typing indicator while the agent works
    await update.message.chat.send_action("typing")

    response = await send_to_openclaw(user_text)

    # Send response back, chunked if necessary
    for chunk in chunk_message(response):
        await update.message.reply_text(chunk)

    logger.info(f"Response sent ({len(response)} chars)")


def main():
    check_config()

    logger.info(f"Starting OpenClaw Telegram bridge")
    logger.info(f"  Gateway: {GATEWAY_URL}")
    logger.info(f"  Agent: {AGENT_ID}")
    logger.info(f"  Allowed user: {ALLOWED_USER_ID}")

    app = Application.builder().token(TELEGRAM_BOT_TOKEN).build()
    app.add_handler(CommandHandler("start", start_command))
    app.add_handler(MessageHandler(filters.TEXT & ~filters.COMMAND, handle_message))

    app.run_polling(allowed_updates=Update.ALL_TYPES)


if __name__ == "__main__":
    main()
