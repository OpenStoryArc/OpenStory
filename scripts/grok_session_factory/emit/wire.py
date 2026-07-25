"""Wire-faithful Grok ACP (updates.jsonl) + events.jsonl emitters."""

from __future__ import annotations

import json
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any
from urllib.parse import quote


@dataclass
class Emitter:
    session_id: str
    cwd: str = "/workspace/demo"
    model_id: str = "grok-4.5"
    t0: int = 1_700_000_000
    _seq: int = 0
    _ts: int = 0
    updates: list[dict] = field(default_factory=list)
    events: list[dict] = field(default_factory=list)
    chat: list[dict] = field(default_factory=list)
    acts_used: list[str] = field(default_factory=list)

    def __post_init__(self) -> None:
        self._ts = self.t0

    def _eid(self) -> str:
        self._seq += 1
        return f"{self.session_id}-{self._seq}"

    def _tick(self, n: int = 1) -> int:
        self._ts += n
        return self._ts

    def _update(
        self,
        session_update: dict,
        *,
        method: str = "session/update",
        meta: dict | None = None,
    ) -> dict:
        params_meta = {"eventId": self._eid(), "agentTimestampMs": self._ts * 1000}
        if meta:
            params_meta.update(meta)
        row = {
            "timestamp": self._tick(),
            "method": method,
            "params": {
                "sessionId": self.session_id,
                "update": session_update,
                "_meta": params_meta,
            },
        }
        self.updates.append(row)
        return row

    def event(self, typ: str, **kwargs: Any) -> None:
        row = {"ts": f"2024-01-01T00:00:{self._ts % 60:02d}.000Z", "type": typ, **kwargs}
        # Use monotonic pseudo-ISO from t0 for readability
        secs = self._ts - self.t0
        row["ts"] = f"2024-01-01T00:{secs // 60:02d}:{secs % 60:02d}.000Z"
        self.events.append(row)

    # ── high-level speech ────────────────────────────────────────────

    def user(self, text: str, prompt_index: int) -> None:
        self.acts_used.append("speak.user")
        self._update(
            {
                "sessionUpdate": "user_message_chunk",
                "content": {"type": "text", "text": text},
                "_meta": {"modelId": self.model_id, "promptIndex": prompt_index},
            },
            meta={"agentTimestampMs": self._ts * 1000},
        )
        self.chat.append({"type": "user", "content": [{"type": "text", "text": text}]})

    def think(self, text: str, prompt_id: str) -> None:
        self.acts_used.append("speak.think")
        self._update(
            {
                "sessionUpdate": "agent_thought_chunk",
                "content": {"type": "text", "text": text},
            },
            meta={
                "promptId": prompt_id,
                "updateType": "AgentThoughtChunk",
                "totalTokens": 1000 + self._seq,
            },
        )
        self.chat.append(
            {
                "type": "reasoning",
                "summary": [{"type": "summary_text", "text": text}],
            }
        )
        self.event("phase_changed", phase="streaming_reasoning")

    def say(self, text: str, prompt_id: str) -> None:
        self.acts_used.append("speak.answer")
        self._update(
            {
                "sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": text},
            },
            meta={
                "promptId": prompt_id,
                "updateType": "AgentMessageChunk",
                "totalTokens": 1200 + self._seq,
            },
        )
        self.chat.append({"type": "assistant", "content": [{"type": "text", "text": text}]})
        self.event("phase_changed", phase="streaming_text")

    def tool(
        self,
        *,
        act_id: str,
        name: str,
        raw_input: dict,
        output: Any,
        is_error: bool,
        prompt_id: str,
        tool_kind: str = "other",
        read_only: bool = True,
        call_id: str | None = None,
    ) -> None:
        self.acts_used.append(act_id)
        cid = call_id or f"call-{uuid.uuid4().hex[:12]}"
        self.event("phase_changed", phase="tool_execution")
        self.event("tool_started", tool_name=name)
        self._update(
            {
                "sessionUpdate": "tool_call",
                "toolCallId": cid,
                "title": name,
                "rawInput": raw_input,
                "_meta": {
                    "x.ai/tool": {
                        "version": 1,
                        "name": name,
                        "kind": tool_kind,
                        "namespace": "grok_build",
                        "label": name,
                        "read_only": read_only,
                    }
                },
            },
            meta={
                "promptId": prompt_id,
                "updateType": "ToolCall",
                "updateParams": {
                    "toolCallId": cid,
                    "title": name,
                    "kind": "Other",
                    "status": "Pending",
                },
            },
        )
        # pending → completed (skip in_progress for density; scenario_02 covers it)
        if isinstance(output, str):
            raw_output: Any = {"type": "Text", "Content": {"content": output}}
        else:
            raw_output = output
        update: dict[str, Any] = {
            "sessionUpdate": "tool_call_update",
            "toolCallId": cid,
            "status": "completed",
            "rawOutput": raw_output,
        }
        if is_error:
            update["isError"] = True
        self._update(update, meta={"updateType": "ToolCallUpdate"})
        self.event(
            "tool_completed",
            tool_name=name,
            duration_ms=12 + self._seq % 50,
            outcome="error" if is_error else "ok",
        )
        self.chat.append(
            {
                "type": "tool_result",
                "tool_name": name,
                "content": output if isinstance(output, str) else json.dumps(output),
                "is_error": is_error,
            }
        )

    def turn_start(self, turn_number: int) -> None:
        self.event(
            "turn_started",
            session_id=self.session_id,
            turn_number=turn_number,
            model_id=self.model_id,
            yolo_mode=True,
            conversation_message_count=turn_number * 2,
            session_relationship="primary",
            schema_version="1.0",
        )
        self.event("loop_started", loop_index=0)
        self.event("phase_changed", phase="waiting_for_model")
        self.event("first_token")

    def turn_end(self, prompt_id: str, loop_count: int = 1) -> None:
        for i in range(1, loop_count):
            self.event("loop_started", loop_index=i)
        self._update(
            {
                "sessionUpdate": "turn_completed",
                "prompt_id": prompt_id,
                "stop_reason": "end_turn",
                "usage": {
                    "inputTokens": 500 + self._seq * 3,
                    "outputTokens": 40 + self._seq,
                    "totalTokens": 540 + self._seq * 4,
                    "cachedReadTokens": 100,
                    "reasoningTokens": 20,
                    "modelCalls": loop_count,
                    "apiDurationMs": 800,
                    "modelUsage": {
                        self.model_id: {
                            "inputTokens": 500,
                            "outputTokens": 40,
                            "totalTokens": 540,
                            "cachedReadTokens": 100,
                            "reasoningTokens": 20,
                            "modelCalls": loop_count,
                            "apiDurationMs": 800,
                        }
                    },
                    "numTurns": 1,
                },
            },
            method="_x.ai/session/update",
        )
        self.event("turn_ended", session_id=self.session_id)

    def write_tree(self, root: Path) -> Path:
        """Write ~/.grok/sessions-shaped tree under root."""
        enc = quote(self.cwd, safe="")
        session_dir = root / "sessions" / enc / self.session_id
        session_dir.mkdir(parents=True, exist_ok=True)
        for name, rows in (
            ("updates.jsonl", self.updates),
            ("events.jsonl", self.events),
            ("chat_history.jsonl", self.chat),
        ):
            path = session_dir / name
            with path.open("w", encoding="utf-8") as f:
                for row in rows:
                    f.write(json.dumps(row, separators=(",", ":"), ensure_ascii=False) + "\n")
        manifest = {
            "session_id": self.session_id,
            "cwd": self.cwd,
            "model_id": self.model_id,
            "acts_used": sorted(set(self.acts_used)),
            "act_instances": len(self.acts_used),
            "update_lines": len(self.updates),
            "event_lines": len(self.events),
            "chat_lines": len(self.chat),
        }
        (session_dir / "MANIFEST.json").write_text(json.dumps(manifest, indent=2) + "\n")
        return session_dir
