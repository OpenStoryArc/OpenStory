/**
 * Mock WebSocket server for performance testing.
 *
 * Lightweight WS server that sends synthetic enriched messages
 * at configurable rates. No real backend needed — the UI connects
 * to this mock and receives a firehose of realistic events.
 *
 * Also serves a minimal HTTP endpoint at /api/sessions so Vite's
 * proxy health-check passes.
 */

import { WebSocketServer, WebSocket } from "ws";
import { createServer, type Server, type IncomingMessage, type ServerResponse } from "http";

// ═══════════════════════════════════════════════════════════════════
// Seeded PRNG — same Mulberry32 as the UI synth generator
// ═══════════════════════════════════════════════════════════════════

function mulberry32(seed: number): () => number {
  let s = seed | 0;
  return () => {
    s = (s + 0x6d2b79f5) | 0;
    let t = Math.imul(s ^ (s >>> 15), 1 | s);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// ═══════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════

export interface MockServerConfig {
  /** Port to listen on. Default: 0 (random). */
  port?: number;
  /** Number of records in initial_state. Default: 100. */
  initialRecords?: number;
  /** Number of sessions in initial_state. Default: 3. */
  sessions?: number;
  /** Rate of enriched messages per second. Default: 10. */
  enrichedRate?: number;
  /** Records per enriched message. Default: 1. */
  recordsPerMessage?: number;
  /** Deterministic seed. Default: 42. */
  seed?: number;
  /** Auto-start streaming on connect? Default: true. */
  autoStream?: boolean;
  /** Approximate payload size per record in bytes. Default: 200 (tiny). */
  payloadSize?: number;
}

export interface MockServer {
  /** Actual port the server is listening on. */
  port: number;
  /** URL for Vite proxy (http://localhost:{port}). */
  url: string;
  /** Start streaming enriched messages to all clients. */
  startStreaming(): void;
  /** Stop streaming. */
  stopStreaming(): void;
  /** Send a burst of N messages immediately. */
  burst(count: number): void;
  /** Disconnect all clients (simulates server crash). */
  disconnectAll(): void;
  /** Shut down the server. */
  close(): Promise<void>;
  /** Number of currently connected clients. */
  clientCount(): number;
  /** Total messages sent since start. */
  messagesSent: number;
}

// ═══════════════════════════════════════════════════════════════════
// Record type distribution (realistic mix)
// ═══════════════════════════════════════════════════════════════════

const RECORD_TYPES = [
  "tool_call", "tool_result", "assistant_message", "user_message",
  "reasoning", "system_event", "error", "turn_end",
] as const;

const TYPE_WEIGHTS: Record<string, number> = {
  tool_call: 30,
  tool_result: 30,
  assistant_message: 15,
  user_message: 5,
  reasoning: 10,
  system_event: 5,
  error: 3,
  turn_end: 2,
};

function pickType(rand: () => number): string {
  const total = Object.values(TYPE_WEIGHTS).reduce((a, b) => a + b, 0);
  let r = rand() * total;
  for (const [type, weight] of Object.entries(TYPE_WEIGHTS)) {
    r -= weight;
    if (r <= 0) return type;
  }
  return "tool_call";
}

// ═══════════════════════════════════════════════════════════════════
// Synthetic record builder
// ═══════════════════════════════════════════════════════════════════

const TOOL_NAMES = ["Bash", "Read", "Write", "Edit", "Grep", "Glob", "Agent"];

function buildPayload(type: string, rand: () => number): object {
  switch (type) {
    case "tool_call":
      return {
        name: TOOL_NAMES[Math.floor(rand() * TOOL_NAMES.length)],
        call_id: `call_${Math.floor(rand() * 1e9)}`,
        input: { command: "echo hello" },
        typed_input: { tool: "bash", command: "echo hello", description: "echo test" },
      };
    case "tool_result":
      return {
        call_id: `call_${Math.floor(rand() * 1e9)}`,
        output: "hello\n",
        is_error: rand() < 0.05,
      };
    case "assistant_message":
      return {
        model: "claude-opus-4-6",
        content: [{ type: "text", text: `Response text block ${Math.floor(rand() * 1000)}` }],
        stop_reason: "end_turn",
      };
    case "user_message":
      return {
        content: `User prompt ${Math.floor(rand() * 1000)}`,
      };
    case "reasoning":
      return {
        encrypted: false,
        summary: [`Thinking about step ${Math.floor(rand() * 100)}`],
        content: null,
      };
    case "system_event":
      return {
        subtype: rand() < 0.5 ? "system.turn.complete" : "system.hook",
        message: "Event processed",
        duration_ms: Math.floor(rand() * 5000),
      };
    case "error":
      return {
        code: "tool_error",
        message: `Error ${Math.floor(rand() * 100)}`,
        details: null,
      };
    case "turn_end":
      return {
        reason: "end_turn",
        duration_ms: Math.floor(rand() * 30000),
      };
    default:
      return {};
  }
}

/** Generate a padding string of roughly `bytes` length. */
function generatePadding(bytes: number, rand: () => number): string {
  if (bytes <= 0) return "";
  // Use a mix of words to make it look like real output
  const words = ["error", "warning", "info", "debug", "trace", "file", "line", "col", "src", "test", "build", "run"];
  const parts: string[] = [];
  let len = 0;
  while (len < bytes) {
    const word = words[Math.floor(rand() * words.length)];
    parts.push(word);
    len += word.length + 1;
  }
  return parts.join(" ").slice(0, bytes);
}

function buildRecord(
  seq: number,
  sessionId: string,
  rand: () => number,
  baseTime: number,
  payloadSize = 0,
): object {
  const type = pickType(rand);
  const ts = new Date(baseTime + seq * 100).toISOString();
  const depth = rand() < 0.2 ? Math.floor(rand() * 5) : 0;
  const payload = buildPayload(type, rand);

  // Pad payload if payloadSize is set
  if (payloadSize > 200) {
    const currentSize = JSON.stringify(payload).length;
    const needed = payloadSize - currentSize;
    if (needed > 0) {
      if (type === "tool_result") {
        (payload as any).output = generatePadding(needed, rand);
      } else if (type === "assistant_message") {
        (payload as any).content = [{ type: "text", text: generatePadding(needed, rand) }];
      } else {
        (payload as any)._padding = generatePadding(needed, rand);
      }
    }
  }

  return {
    id: `evt-${seq}-${Math.floor(rand() * 1e9)}`,
    seq,
    session_id: sessionId,
    timestamp: ts,
    record_type: type,
    payload,
    agent_id: depth > 0 ? `agent-${Math.floor(rand() * 10)}` : null,
    is_sidechain: depth > 0,
    depth,
    parent_uuid: depth > 0 ? `evt-${seq - 1}-parent` : null,
    truncated: payloadSize > 2048,
    payload_bytes: Math.max(Math.floor(rand() * 2000), payloadSize),
  };
}

// ═══════════════════════════════════════════════════════════════════
// Filter names (must match server FILTER_NAMES)
// ═══════════════════════════════════════════════════════════════════

const FILTER_NAMES = [
  "all", "user", "assistant", "tool_calls", "tool_results",
  "reasoning", "system", "errors", "bash", "read", "write",
  "edit", "grep", "glob", "agent", "file_create", "narrative",
];

function computeFilterCounts(records: object[]): Record<string, Record<string, number>> {
  const perSession: Record<string, Record<string, number>> = {};
  for (const r of records) {
    const rec = r as any;
    const sid = rec.session_id;
    if (!perSession[sid]) perSession[sid] = {};
    const sc = perSession[sid];
    sc["all"] = (sc["all"] ?? 0) + 1;
    const rt = rec.record_type;
    if (rt === "user_message") sc["user"] = (sc["user"] ?? 0) + 1;
    if (rt === "assistant_message") { sc["assistant"] = (sc["assistant"] ?? 0) + 1; sc["narrative"] = (sc["narrative"] ?? 0) + 1; }
    if (rt === "tool_call") sc["tool_calls"] = (sc["tool_calls"] ?? 0) + 1;
    if (rt === "tool_result") sc["tool_results"] = (sc["tool_results"] ?? 0) + 1;
    if (rt === "reasoning") { sc["reasoning"] = (sc["reasoning"] ?? 0) + 1; sc["narrative"] = (sc["narrative"] ?? 0) + 1; }
    if (rt === "system_event") sc["system"] = (sc["system"] ?? 0) + 1;
    if (rt === "error") sc["errors"] = (sc["errors"] ?? 0) + 1;
    if (rt === "tool_call") {
      const name = (rec.payload?.name ?? "").toLowerCase();
      if (name === "bash") sc["bash"] = (sc["bash"] ?? 0) + 1;
      if (name === "read") sc["read"] = (sc["read"] ?? 0) + 1;
      if (name === "write") sc["write"] = (sc["write"] ?? 0) + 1;
      if (name === "edit") sc["edit"] = (sc["edit"] ?? 0) + 1;
      if (name === "grep") sc["grep"] = (sc["grep"] ?? 0) + 1;
      if (name === "glob") sc["glob"] = (sc["glob"] ?? 0) + 1;
      if (name === "agent") sc["agent"] = (sc["agent"] ?? 0) + 1;
    }
  }
  return perSession;
}

// ═══════════════════════════════════════════════════════════════════
// Server
// ═══════════════════════════════════════════════════════════════════

export async function createMockServer(config: MockServerConfig = {}): Promise<MockServer> {
  const {
    port = 0,
    initialRecords = 100,
    sessions = 3,
    enrichedRate = 10,
    recordsPerMessage = 1,
    seed = 42,
    autoStream = true,
    payloadSize = 0,
  } = config;

  const rand = mulberry32(seed);
  const baseTime = Date.now();

  // Generate session IDs
  const sessionIds = Array.from({ length: sessions }, (_, i) =>
    `session-${String(i).padStart(3, "0")}-${Math.floor(rand() * 1e9)}`
  );

  // Generate initial records
  const initialRecs: object[] = [];
  for (let i = 0; i < initialRecords; i++) {
    const sid = sessionIds[i % sessions];
    initialRecs.push(buildRecord(i, sid, rand, baseTime, payloadSize));
  }

  const filterCounts = computeFilterCounts(initialRecs);

  // Build initial_state message
  const initialState = JSON.stringify({
    kind: "initial_state",
    records: initialRecs,
    filter_counts: filterCounts,
    patterns: [],
    session_labels: Object.fromEntries(
      sessionIds.map((sid, i) => [sid, { label: `Session ${i}`, branch: "master" }])
    ),
    agent_labels: {},
  });

  // HTTP + WS server
  const httpServer = createServer((req: IncomingMessage, res: ServerResponse) => {
    if (req.url === "/api/sessions") {
      res.writeHead(200, { "Content-Type": "application/json" });
      const sessionsArr = sessionIds.map((sid, i) => ({
        session_id: sid,
        status: "ongoing",
        event_count: initialRecords / sessions,
        project_id: "project-perf-test",
        project_name: "perf-test",
      }));
      res.end(JSON.stringify({ sessions: sessionsArr, total: sessionsArr.length }));
    } else {
      res.writeHead(404);
      res.end("Not found");
    }
  });

  const wss = new WebSocketServer({ server: httpServer, path: "/ws" });

  let seq = initialRecords;
  let streamInterval: ReturnType<typeof setInterval> | null = null;
  let totalSent = 0;

  function sendToAll(data: string) {
    for (const client of wss.clients) {
      if (client.readyState === WebSocket.OPEN) {
        client.send(data);
      }
    }
    totalSent++;
  }

  function buildEnrichedMessage(): string {
    const sid = sessionIds[Math.floor(rand() * sessions)];
    const records: object[] = [];
    for (let i = 0; i < recordsPerMessage; i++) {
      records.push(buildRecord(seq++, sid, rand, baseTime, payloadSize));
    }

    // Compute filter deltas for this batch
    const deltas: Record<string, number> = {};
    for (const r of records) {
      const rec = r as any;
      deltas["all"] = (deltas["all"] ?? 0) + 1;
      const rt = rec.record_type;
      if (rt === "user_message") deltas["user"] = (deltas["user"] ?? 0) + 1;
      if (rt === "assistant_message") { deltas["assistant"] = (deltas["assistant"] ?? 0) + 1; deltas["narrative"] = (deltas["narrative"] ?? 0) + 1; }
      if (rt === "tool_call") deltas["tool_calls"] = (deltas["tool_calls"] ?? 0) + 1;
      if (rt === "tool_result") deltas["tool_results"] = (deltas["tool_results"] ?? 0) + 1;
      if (rt === "reasoning") { deltas["reasoning"] = (deltas["reasoning"] ?? 0) + 1; deltas["narrative"] = (deltas["narrative"] ?? 0) + 1; }
      if (rt === "system_event") deltas["system"] = (deltas["system"] ?? 0) + 1;
      if (rt === "error") deltas["errors"] = (deltas["errors"] ?? 0) + 1;
    }

    return JSON.stringify({
      kind: "enriched",
      session_id: sid,
      records,
      ephemeral: [],
      filter_deltas: deltas,
    });
  }

  // Send initial_state to new connections
  wss.on("connection", (ws) => {
    ws.send(initialState);
  });

  // Start listening
  await new Promise<void>((resolve, reject) => {
    httpServer.on("error", reject);
    httpServer.listen(port, "127.0.0.1", () => resolve());
  });

  const actualPort = (httpServer.address() as any).port;

  const server: MockServer = {
    port: actualPort,
    url: `http://127.0.0.1:${actualPort}`,
    messagesSent: 0,

    startStreaming() {
      if (streamInterval) return;
      const intervalMs = 1000 / enrichedRate;
      streamInterval = setInterval(() => {
        sendToAll(buildEnrichedMessage());
      }, intervalMs);
    },

    stopStreaming() {
      if (streamInterval) {
        clearInterval(streamInterval);
        streamInterval = null;
      }
    },

    burst(count: number) {
      for (let i = 0; i < count; i++) {
        sendToAll(buildEnrichedMessage());
      }
    },

    disconnectAll() {
      for (const client of wss.clients) {
        client.close(1000, "Mock server disconnect");
      }
    },

    clientCount() {
      return wss.clients.size;
    },

    async close() {
      server.stopStreaming();
      // Force-close all WS connections first
      for (const client of wss.clients) {
        try { client.terminate(); } catch { /* ignore */ }
      }
      wss.close();
      // Close HTTP server with a timeout to prevent hanging
      await Promise.race([
        new Promise<void>((resolve) => httpServer.close(() => resolve())),
        new Promise<void>((resolve) => setTimeout(resolve, 2000)),
      ]);
    },

    get messagesSent() {
      return totalSent;
    },
    set messagesSent(_: number) {
      // read-only externally
    },
  };

  if (autoStream) {
    server.startStreaming();
  }

  return server;
}
