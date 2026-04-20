#!/usr/bin/env python3
"""
hermes_write_timeline.py — Visualize the precise file write operations
during a Hermes session.

Shows: when each message is added, when _persist_session() fires,
what the file looks like at each write, and how the snapshot-diff
watcher would observe the changes.

Usage:
    python3 scripts/hermes_write_timeline.py ~/.hermes/sessions/session_*.json
    open /tmp/hermes-write-timeline.html
"""

import json
import os
import sys
from datetime import datetime
from pathlib import Path


def esc(s: str) -> str:
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def build_html(data: dict) -> str:
    msgs = data.get("messages", [])
    sid = data["session_id"]
    model = data.get("model", "?")
    start = data.get("session_start", "")
    end = data.get("last_updated", "")

    try:
        t_start = datetime.fromisoformat(start)
        t_end = datetime.fromisoformat(end)
        duration = max((t_end - t_start).total_seconds(), 1)
    except Exception:
        duration = 30

    # Build timeline events
    timeline_html = ""
    write_num = 0
    cumulative_size = 0
    msg_count = 0
    watcher_diffs = []
    prev_visible = 0

    for i, msg in enumerate(msgs):
        role = msg.get("role", "?")
        content = msg.get("content", "")
        tc = msg.get("tool_calls", [])
        msg_count += 1
        msg_size = len(json.dumps(msg))
        cumulative_size += msg_size
        t_frac = (i / max(len(msgs) - 1, 1)) * duration

        if role == "user":
            timeline_html += f"""
            <div class="event user">
                <div class="event-time">~{t_frac:.0f}s</div>
                <div class="event-label" style="color:#7aa2f7">USER PROMPT</div>
                <div class="event-detail">"{esc(content[:60])}{'...' if len(content) > 60 else ''}"</div>
                <div class="event-detail dim">messages.append(msg) — in-memory only, no file write</div>
            </div>"""

        elif role == "assistant":
            tool_names = [t.get("function", {}).get("name", "?") for t in tc]
            has_tools = bool(tc)
            write_num += 1
            phase = "EVAL → tool dispatch" if has_tools else "EVAL → final answer"
            tools_str = f"tools: {tool_names}" if has_tools else f'"{esc(content[:50])}..."'
            file_kb = (cumulative_size + 35000) // 1024

            timeline_html += f"""
            <div class="event assistant">
                <div class="event-time">~{t_frac:.0f}s</div>
                <div class="event-label" style="color:#9ece6a">{phase}</div>
                <div class="event-detail">{tools_str}</div>
            </div>
            <div class="event write">
                <div class="event-label" style="color:#f7768e">FILE WRITE #{write_num} — _persist_session()</div>
                <div class="write-box">
                    <div class="write-step"><code>1.</code> mkstemp(<code>.session_{sid[:8]}...tmp</code>) — temp file in same dir</div>
                    <div class="write-step"><code>2.</code> json.dump({{session_id, model, system_prompt, tools, <strong>messages: [{msg_count} msgs]</strong>}})</div>
                    <div class="write-step"><code>3.</code> f.flush() + os.fsync(fd) — force bytes to disk</div>
                    <div class="write-step"><code>4.</code> os.replace(tmp → <code>session_{sid}.json</code>) <span style="color:#9ece6a">← atomic POSIX rename</span></div>
                    <div class="write-size">file now: ~{file_kb} KB ({msg_count} messages, ~{cumulative_size//1024} KB content + ~35 KB envelope)</div>
                </div>
            </div>"""

            # Track watcher diff
            curr_visible = sum(1 for m in msgs[:i + 1] if m.get("role") != "system")
            delta = curr_visible - prev_visible
            watcher_diffs.append((prev_visible, curr_visible, delta))
            prev_visible = curr_visible

        elif role == "tool":
            tid = msg.get("tool_call_id", "?")
            tool_summary = content[:60]
            try:
                parsed = json.loads(content)
                if isinstance(parsed, dict):
                    if "error" in parsed and parsed["error"]:
                        tool_summary = f"ERROR: {parsed['error'][:40]}"
                    elif "output" in parsed:
                        tool_summary = f"output: {str(parsed['output'])[:40]}"
                    elif "content" in parsed:
                        tool_summary = f"content: {str(parsed['content'])[:40]}"
                    elif "success" in parsed:
                        tool_summary = f"success={parsed['success']}"
                    elif "bytes_written" in parsed:
                        tool_summary = f"bytes_written={parsed['bytes_written']}"
            except Exception:
                pass

            timeline_html += f"""
            <div class="event tool">
                <div class="event-time">~{t_frac:.0f}s</div>
                <div class="event-label" style="color:#bb9af7">TOOL RESULT</div>
                <div class="event-detail">{esc(tool_summary)}</div>
                <div class="event-detail dim">messages.append(msg) — in-memory only, <strong>NO file write</strong></div>
            </div>"""

    # Watcher diff visualization
    diff_html = ""
    for prev, curr, delta in watcher_diffs:
        diff_html += f"""
        <div class="diff-row">
            <span class="diff-old">prev: {prev} msgs</span>
            <span class="diff-arrow">→</span>
            <span class="diff-new">curr: {curr} msgs</span>
            <span class="diff-arrow">→</span>
            <span class="diff-emit">emit {delta} new CloudEvents</span>
            <span class="diff-slice">(messages[{prev}:{curr}])</span>
        </div>"""

    return f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Hermes Write Timeline — {sid}</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{ background: #1a1b26; color: #c0caf5; font-family: 'JetBrains Mono', monospace; font-size: 12px; padding: 24px; max-width: 900px; margin: 0 auto; }}
  h1 {{ color: #7aa2f7; font-size: 16px; margin-bottom: 4px; }}
  h2 {{ color: #bb9af7; font-size: 14px; margin: 24px 0 10px; border-bottom: 1px solid #3b4261; padding-bottom: 4px; }}
  .subtitle {{ color: #565f89; margin-bottom: 16px; }}
  .legend {{ display: flex; gap: 16px; margin: 12px 0; flex-wrap: wrap; }}
  .legend-item {{ display: flex; align-items: center; gap: 4px; font-size: 10px; }}
  .legend-dot {{ width: 10px; height: 10px; border-radius: 50%; }}

  .timeline {{ position: relative; margin: 20px 0 20px 100px; }}
  .timeline::before {{ content: ''; position: absolute; left: 0; top: 0; bottom: 0; width: 2px; background: #3b4261; }}
  .event {{ position: relative; padding: 6px 0 6px 24px; }}
  .event::before {{ content: ''; position: absolute; left: -5px; top: 10px; width: 12px; height: 12px; border-radius: 50%; }}
  .event.user::before {{ background: #7aa2f7; }}
  .event.assistant::before {{ background: #9ece6a; }}
  .event.tool::before {{ background: #bb9af7; }}
  .event.write::before {{ background: #f7768e; box-shadow: 0 0 8px #f7768e66; }}
  .event-label {{ font-weight: bold; font-size: 11px; }}
  .event-detail {{ color: #9aa5ce; font-size: 10px; margin-top: 2px; }}
  .event-detail.dim {{ color: #565f89; }}
  .event-time {{ position: absolute; left: -100px; top: 8px; color: #565f89; font-size: 10px; width: 90px; text-align: right; }}

  .write-box {{ background: #f7768e10; border: 1px solid #f7768e33; border-radius: 6px; padding: 8px 12px; margin: 4px 0 4px 24px; }}
  .write-step {{ font-size: 10px; color: #c0caf5; margin: 2px 0; }}
  .write-step code {{ color: #f7768e; }}
  .write-size {{ font-size: 10px; color: #e0af68; margin-top: 4px; font-weight: bold; }}

  .box {{ background: #24283b; border: 1px solid #3b4261; border-radius: 8px; padding: 16px; margin: 16px 0; }}
  .box h3 {{ font-size: 12px; margin-bottom: 10px; }}
  .box p {{ font-size: 11px; line-height: 1.6; }}
  .box ul {{ margin: 8px 0 8px 20px; font-size: 11px; line-height: 1.6; }}

  .watcher-box {{ border-color: #9ece6a33; }}
  .watcher-box h3 {{ color: #9ece6a; }}
  .diff-row {{ margin: 4px 0; font-size: 11px; }}
  .diff-old {{ color: #565f89; }}
  .diff-new {{ color: #9ece6a; font-weight: bold; }}
  .diff-arrow {{ color: #3b4261; margin: 0 6px; }}
  .diff-emit {{ color: #e0af68; }}
  .diff-slice {{ color: #565f89; margin-left: 4px; }}
  .diff-note {{ margin-top: 10px; color: #565f89; font-size: 10px; }}

  .race-box {{ border-color: #7aa2f733; }}
  .race-box h3 {{ color: #7aa2f7; }}

  .perf-box {{ border-color: #e0af6833; }}
  .perf-box h3 {{ color: #e0af68; }}

  .footer {{ margin-top: 24px; color: #3b4261; font-size: 10px; text-align: center; }}
</style>
</head>
<body>

<h1>Hermes Write Timeline — Precise File Operations</h1>
<div class="subtitle">{sid} · {len(msgs)} messages · {duration:.0f}s · {model} · {write_num} file writes</div>

<div class="legend">
  <div class="legend-item"><div class="legend-dot" style="background:#7aa2f7"></div> User prompt (no write)</div>
  <div class="legend-item"><div class="legend-dot" style="background:#9ece6a"></div> Assistant response (EVAL)</div>
  <div class="legend-item"><div class="legend-dot" style="background:#bb9af7"></div> Tool result (no write)</div>
  <div class="legend-item"><div class="legend-dot" style="background:#f7768e;box-shadow:0 0 6px #f7768e66"></div> File write (_persist_session)</div>
</div>

<h2>Timeline — {len(msgs)} messages, {write_num} writes in {duration:.0f}s ({write_num/duration:.2f} writes/sec)</h2>
<div class="timeline">
{timeline_html}
</div>

<h2>What the snapshot-diff watcher sees</h2>
<div class="box watcher-box">
    <h3>Diff at each write point</h3>
    {diff_html}
    <div class="diff-note">
        <strong>Key insight:</strong> The watcher doesn't need to see every write.
        If it misses write #3 and reads at write #4, the diff still produces the correct
        CloudEvents — it just emits messages 3+4 together. The diff is <strong>idempotent and convergent</strong>.
    </div>
</div>

<h2>Race condition analysis</h2>
<div class="box race-box">
    <h3>Can the watcher interfere with Hermes?</h3>
    <p><strong>No.</strong> The watcher only reads. Hermes only writes. There is no shared lock.</p>
    <p style="margin-top: 8px;">The atomic rename (<code>os.replace</code>) guarantees:</p>
    <ul>
        <li>Watcher opens file → gets fd pointing to <strong>inode A</strong></li>
        <li>Hermes writes new version → temp file, renames over path → path now points to <strong>inode B</strong></li>
        <li>Watcher's fd <strong>still reads inode A</strong> (old content, complete, valid)</li>
        <li>Watcher closes fd → inode A freed by OS</li>
        <li>Next poll: watcher opens path → gets <strong>inode B</strong> (new content)</li>
    </ul>
    <p style="margin-top: 8px; color: #9ece6a;"><strong>The watcher never sees partial data.</strong> POSIX guarantee.</p>
    <p style="margin-top: 8px; color: #e0af68;"><strong>Performance impact on Hermes: zero.</strong> Read and write operate on different inodes after the rename. No lock, no blocking, no shared state.</p>
</div>

<div class="box perf-box">
    <h3>Performance profile</h3>
    <ul>
        <li>Write frequency: <strong>{write_num/duration:.2f} writes/sec</strong> (one per assistant response)</li>
        <li>Tool results do NOT trigger writes — they accumulate in memory until the next LLM call</li>
        <li>File parse cost at current sizes: <strong>&lt;1ms</strong> (60-90 KB JSON)</li>
        <li>A 1-second poll interval catches every write at observed rates</li>
        <li>Even skipping intermediate states produces correct output (diff convergence)</li>
    </ul>
</div>

<div class="footer">
    Generated by hermes_write_timeline.py — OpenStory × Hermes Agent integration
</div>

</body>
</html>"""


def main():
    if len(sys.argv) < 2:
        print("Usage: python3 hermes_write_timeline.py <session_*.json>")
        sys.exit(1)

    data = json.load(open(sys.argv[1]))
    html = build_html(data)

    out = "/tmp/hermes-write-timeline.html"
    Path(out).write_text(html)
    print(f"Written to {out}")


if __name__ == "__main__":
    main()
