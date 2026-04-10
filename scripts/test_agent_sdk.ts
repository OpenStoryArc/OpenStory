/**
 * End-to-end test: Open Story captures events from a Claude Agent SDK session.
 *
 * Usage:
 *   # Install dependencies first:
 *   npm install @anthropic-ai/claude-code
 *
 *   # Run with tsx (or ts-node):
 *   npx tsx scripts/test_agent_sdk.ts
 *
 *   # With custom port:
 *   OS_PORT=3099 npx tsx scripts/test_agent_sdk.ts
 *
 * Prerequisites:
 *   - ANTHROPIC_API_KEY set in environment
 *   - Open Story server running (or this script starts it)
 *   - Claude Code hooks configured (~/.claude/settings.json)
 *
 * What this does:
 *   1. Spawns Claude Code as a subprocess via the SDK's query() function
 *   2. Sends a simple task (read files, create output)
 *   3. Verifies events were captured via Open Story's /api/sessions
 *   4. Reports pass/fail
 */

import { query, type MessageParam } from "@anthropic-ai/claude-code";

const OS_PORT = process.env.OS_PORT || "3002";
const BASE_URL = `http://localhost:${OS_PORT}`;

// --- Helpers ---

async function fetchJson(path: string): Promise<unknown> {
  const resp = await fetch(`${BASE_URL}${path}`);
  if (!resp.ok) throw new Error(`HTTP ${resp.status} on ${path}`);
  return resp.json();
}

function pass(msg: string) {
  console.log(`\x1b[32mPASS: ${msg}\x1b[0m`);
}

function fail(msg: string) {
  console.error(`\x1b[31mFAIL: ${msg}\x1b[0m`);
}

// --- Main ---

async function main() {
  if (!process.env.ANTHROPIC_API_KEY) {
    fail("ANTHROPIC_API_KEY not set");
    process.exit(1);
  }

  // Verify Open Story is running
  try {
    await fetchJson("/api/sessions");
  } catch {
    fail(`Open Story not reachable at ${BASE_URL}. Start it first.`);
    process.exit(1);
  }

  console.log("==> Running Claude Agent SDK task...");

  // Use the claude-code SDK's query() function
  // This spawns Claude Code as a subprocess — hooks fire normally
  const messages: MessageParam[] = [];
  let resultText = "";

  try {
    const result = await query({
      prompt:
        "Read any files in the current directory. Then create a file called sdk_test_output.txt with the text 'SDK integration test passed'. Reply with a summary of what you did.",
      options: {
        maxTurns: 5,
        allowedTools: ["Read", "Write", "Glob", "Grep"],
      },
    });

    resultText =
      typeof result === "string"
        ? result
        : Array.isArray(result)
          ? result
              .filter((b: { type: string }) => b.type === "text")
              .map((b: { text: string }) => b.text)
              .join("\n")
          : JSON.stringify(result);

    console.log(`Claude result (first 300 chars): ${resultText.slice(0, 300)}`);
  } catch (err) {
    fail(`Claude SDK query failed: ${err}`);
    process.exit(1);
  }

  // Give hooks a moment to deliver
  await new Promise((r) => setTimeout(r, 3000));

  // --- Verify events captured ---
  console.log("==> Checking for captured sessions...");

  let results = 0;
  let failures = 0;

  const sessionsResp = (await fetchJson("/api/sessions")) as {
    sessions: Array<{ session_id: string; event_count?: number }>;
    total: number;
  };
  const sessions = sessionsResp.sessions;
  console.log(`Sessions found: ${sessions.length}`);

  // Test 1: At least one session
  if (sessions.length > 0) {
    pass(`Captured ${sessions.length} session(s)`);
    results++;
  } else {
    fail("No sessions captured");
    failures++;
  }

  // Test 2: Session has events
  if (sessions.length > 0) {
    const sessionId = sessions[0].session_id;
    const events = (await fetchJson(
      `/api/sessions/${sessionId}/events`
    )) as Array<{ subtype?: string }>;

    if (events.length > 0) {
      pass(`Session ${sessionId} has ${events.length} events`);
      results++;
    } else {
      fail(`Session ${sessionId} has no events`);
      failures++;
    }

    // Test 3: Has assistant events
    const subtypes = new Set(
      events.map((e) => e.subtype?.split(".").slice(0, 2).join(".") || "")
    );
    if (subtypes.has("message.assistant")) {
      pass(`Found assistant message events (subtypes: ${[...subtypes].join(", ")})`);
      results++;
    } else {
      fail(`No assistant events (subtypes: ${[...subtypes].join(", ")})`);
      failures++;
    }
  }

  // --- Summary ---
  console.log("");
  console.log("================================");
  console.log(`Results: ${results} passed, ${failures} failed`);
  console.log("================================");

  process.exit(failures === 0 ? 0 : 1);
}

main().catch((err) => {
  console.error("Unhandled error:", err);
  process.exit(1);
});
