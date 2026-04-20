"""Telegram <-> OpenClaw bridge bot.

Receives messages from Telegram, forwards them to the OpenClaw gateway
via its OpenAI-compatible HTTP API, and sends the agent's responses back.

Environment variables:
    TELEGRAM_BOT_TOKEN       - Bot token from BotFather (required)
    TELEGRAM_ALLOWED_USER_IDS - Comma-separated Telegram user IDs allowed to use the bot (required)
    OPENCLAW_GATEWAY_URL     - OpenClaw gateway URL (default: http://openclaw:18789)
    OPENCLAW_AGENT_ID        - Agent ID to message (default: main)
    OPENCLAW_AUTH_TOKEN       - Gateway auth token (required)
"""

import asyncio
import json
import logging
import os
import sys

import httpx
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
def _parse_allowed_ids():
    raw = os.environ.get("TELEGRAM_ALLOWED_USER_IDS", os.environ.get("TELEGRAM_ALLOWED_USER_ID", ""))
    return {int(x.strip()) for x in raw.split(",") if x.strip()}

ALLOWED_USER_IDS = _parse_allowed_ids()
GATEWAY_URL = os.environ.get("OPENCLAW_GATEWAY_URL", "http://openclaw:18789")
AGENT_ID = os.environ.get("OPENCLAW_AGENT_ID", "main")
AUTH_TOKEN = os.environ.get("OPENCLAW_AUTH_TOKEN", "")

TELEGRAM_MAX_MESSAGE_LENGTH = 4096


def check_config():
    if not TELEGRAM_BOT_TOKEN:
        logger.error("TELEGRAM_BOT_TOKEN is required")
        sys.exit(1)
    if not ALLOWED_USER_IDS:
        logger.error("TELEGRAM_ALLOWED_USER_IDS is required (comma-separated user IDs)")
        sys.exit(1)
    if not AUTH_TOKEN:
        logger.warning("OPENCLAW_AUTH_TOKEN not set — requests will likely fail auth")


def is_authorized(user_id: int) -> bool:
    return user_id in ALLOWED_USER_IDS


def chunk_message(text: str) -> list[str]:
    """Split text into chunks that fit Telegram's message length limit."""
    if len(text) <= TELEGRAM_MAX_MESSAGE_LENGTH:
        return [text]

    chunks = []
    while text:
        if len(text) <= TELEGRAM_MAX_MESSAGE_LENGTH:
            chunks.append(text)
            break
        split_at = text.rfind("\n", 0, TELEGRAM_MAX_MESSAGE_LENGTH)
        if split_at == -1 or split_at < TELEGRAM_MAX_MESSAGE_LENGTH // 2:
            split_at = TELEGRAM_MAX_MESSAGE_LENGTH
        chunks.append(text[:split_at])
        text = text[split_at:].lstrip("\n")
    return chunks


async def send_to_openclaw(message: str, user_id: str) -> str:
    """Send a message to OpenClaw via the OpenAI-compatible HTTP API."""
    url = f"{GATEWAY_URL}/v1/chat/completions"
    headers = {"Content-Type": "application/json"}
    if AUTH_TOKEN:
        headers["Authorization"] = f"Bearer {AUTH_TOKEN}"
    payload = {
        "model": f"openclaw/{AGENT_ID}",
        "messages": [{"role": "user", "content": message}],
        "user": user_id,
    }

    try:
        async with httpx.AsyncClient(timeout=300) as client:
            resp = await client.post(url, json=payload, headers=headers)
            if resp.status_code != 200:
                logger.error(f"OpenClaw returned {resp.status_code}: {resp.text[:500]}")
                return f"[Error: OpenClaw returned {resp.status_code}]"
            data = resp.json()
            choices = data.get("choices", [])
            if choices:
                return choices[0].get("message", {}).get("content", "[No content]")
            return "[No response from agent]"
    except httpx.TimeoutException:
        return "[Request timed out — agent may still be working]"
    except (httpx.ConnectError, OSError) as e:
        return f"[Could not reach OpenClaw gateway: {e}]"


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

    await update.message.chat.send_action("typing")

    response = await send_to_openclaw(user_text, str(user.id))

    for chunk in chunk_message(response):
        await update.message.reply_text(chunk)

    logger.info(f"Response sent ({len(response)} chars)")


def main():
    check_config()

    logger.info(f"Starting OpenClaw Telegram bridge (HTTP mode)")
    logger.info(f"  Gateway: {GATEWAY_URL}")
    logger.info(f"  Agent: {AGENT_ID}")
    logger.info(f"  Allowed users: {ALLOWED_USER_IDS}")

    app = Application.builder().token(TELEGRAM_BOT_TOKEN).build()
    app.add_handler(CommandHandler("start", start_command))
    app.add_handler(MessageHandler(filters.TEXT & ~filters.COMMAND, handle_message))

    app.run_polling(allowed_updates=Update.ALL_TYPES)


def run_tests():
    """Self-test for pure functions. Run with: python bot.py --test"""
    passed = 0
    failed = 0

    def assert_eq(actual, expected, label):
        nonlocal passed, failed
        if actual == expected:
            passed += 1
            print(f"  \033[32m✓\033[0m {label}")
        else:
            failed += 1
            print(f"  \033[31m✗\033[0m {label}")
            print(f"    expected: {expected!r}")
            print(f"    actual:   {actual!r}")

    print("\nchunk_message:")
    assert_eq(chunk_message("hello"), ["hello"], "short message returns single chunk")
    assert_eq(chunk_message(""), [""], "empty string returns single chunk")

    exact = "a" * TELEGRAM_MAX_MESSAGE_LENGTH
    assert_eq(chunk_message(exact), [exact], "message exactly at limit is one chunk")

    over = "a" * (TELEGRAM_MAX_MESSAGE_LENGTH + 1)
    chunks = chunk_message(over)
    assert_eq(len(chunks), 2, "one over limit produces two chunks")
    assert_eq(len(chunks[0]), TELEGRAM_MAX_MESSAGE_LENGTH, "first chunk is exactly at limit")
    assert_eq(chunks[1], "a", "second chunk is the remainder")

    first_part = "x" * (TELEGRAM_MAX_MESSAGE_LENGTH - 10)
    second_part = "y" * 100
    with_newline = first_part + "\n" + second_part
    chunks = chunk_message(with_newline)
    assert_eq(chunks[0], first_part, "splits at newline before limit")
    assert_eq(chunks[1], second_part, "second chunk starts after newline")

    print("\nis_authorized:")
    global ALLOWED_USER_IDS
    original = ALLOWED_USER_IDS
    ALLOWED_USER_IDS = {12345, 67890}
    assert_eq(is_authorized(12345), True, "first allowed user is authorized")
    assert_eq(is_authorized(67890), True, "second allowed user is authorized")
    assert_eq(is_authorized(99999), False, "non-matching user ID is rejected")
    assert_eq(is_authorized(0), False, "zero user ID is rejected")
    ALLOWED_USER_IDS = original

    print(f"\n{'─' * 40}")
    total = passed + failed
    if failed == 0:
        print(f"\033[32m{passed}/{total} tests passed\033[0m\n")
    else:
        print(f"\033[31m{failed}/{total} tests FAILED\033[0m\n")
        sys.exit(1)


if __name__ == "__main__":
    if "--test" in sys.argv:
        run_tests()
    else:
        main()
