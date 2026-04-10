#!/usr/bin/env python3
"""Generate the full Story visualization HTML from the OpenStory API.

Fetches StructuralTurns and sentence patterns for a session,
renders the prototype HTML (same structure as render-html.ts).

Usage:
    python3 scripts/story_html.py <session_id>
    python3 scripts/story_html.py <session_id> --open
"""

import json
import sys
import urllib.request
from html import escape

API = "http://localhost:3002"


def fetch(url):
    with urllib.request.urlopen(url) as resp:
        return json.loads(resp.read())


def main():
    if len(sys.argv) < 2:
        sessions = fetch(f"{API}/api/sessions")["sessions"]
        print("Sessions:")
        for s in sessions[:10]:
            label = (s.get("first_prompt") or s["session_id"])[:60]
            print(f"  {s['session_id'][:12]}  {s.get('event_count',0):4d} events  {label}")
        print("\nUsage: python3 scripts/story_html.py <session_id>")
        return

    sid = sys.argv[1]
    # Allow short IDs
    if len(sid) < 36:
        sessions = fetch(f"{API}/api/sessions")["sessions"]
        matches = [s for s in sessions if s["session_id"].startswith(sid)]
        if not matches:
            print(f"No session matching '{sid}'")
            return
        sid = matches[0]["session_id"]

    turns_data = fetch(f"{API}/api/sessions/{sid}/turns")
    turns = turns_data.get("turns", [])

    sentences_data = fetch(f"{API}/api/sessions/{sid}/patterns?type=turn.sentence")
    sentences = {p["metadata"]["turn"]: p for p in sentences_data.get("patterns", [])}

    print(f"Session {sid[:12]}: {len(turns)} turns, {len(sentences)} sentences")

    html = render_page(sid, turns, sentences)
    out = "/tmp/story.html"
    with open(out, "w") as f:
        f.write(html)
    print(f"Written to {out}")

    if "--open" in sys.argv:
        import subprocess
        subprocess.run(["open", out])


def render_page(sid, turns, sentences):
    turn_cards = "\n".join(render_turn(t, sentences.get(t["turn_number"])) for t in turns)

    total_applies = sum(len(t.get("applies", [])) for t in turns)
    terminal = sum(1 for t in turns if t.get("is_terminal"))
    continued = len(turns) - terminal

    return f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Story: {escape(sid[:12])}</title>
<style>
  * {{ box-sizing: border-box; margin: 0; padding: 0; }}
  body {{ background: #1a1b26; color: #c0caf5; font-family: 'JetBrains Mono', 'Fira Code', monospace; font-size: 13px; line-height: 1.6; padding: 24px; max-width: 960px; margin: 0 auto; }}
  h1 {{ color: #7aa2f7; font-size: 18px; margin-bottom: 4px; }}
  .subtitle {{ color: #565f89; font-size: 12px; margin-bottom: 20px; }}
  .stats-bar {{ display: flex; gap: 12px; flex-wrap: wrap; margin-bottom: 24px; padding: 12px 16px; background: #24283b; border-radius: 8px; }}
  .stat {{ color: #a9b1d6; font-size: 12px; }}
  .stat b {{ color: #c0caf5; }}
  .legend {{ display: flex; gap: 14px; margin-bottom: 20px; flex-wrap: wrap; }}
  .legend-item {{ display: flex; align-items: center; gap: 4px; font-size: 11px; color: #565f89; }}
  .legend-dot {{ width: 10px; height: 10px; border-radius: 2px; }}

  .turn {{ margin-bottom: 8px; border-radius: 8px; background: #1f2335; overflow: hidden; border: 1px solid #2a2e42; }}
  .turn:hover {{ border-color: #3b4261; }}
  .turn-header {{ display: flex; justify-content: space-between; align-items: center; padding: 8px 14px; background: #24283b; cursor: pointer; }}
  .turn-left {{ display: flex; align-items: center; gap: 10px; }}
  .turn-num {{ color: #7aa2f7; font-weight: bold; font-size: 12px; }}
  .badge {{ font-size: 9px; padding: 2px 7px; border-radius: 3px; font-weight: bold; text-transform: uppercase; letter-spacing: 0.5px; }}
  .badge.terminate {{ background: #9ece6a18; color: #9ece6a; border: 1px solid #9ece6a33; }}
  .badge.continue {{ background: #e0af6818; color: #e0af68; border: 1px solid #e0af6833; }}
  .turn-body {{ padding: 10px 14px; }}
  .turn-footer {{ display: flex; justify-content: space-between; padding: 4px 14px 8px; font-size: 11px; color: #565f89; }}

  .phase {{ padding: 6px 10px; margin: 4px 0; border-left: 3px solid; border-radius: 0 4px 4px 0; background: #24283b; }}
  .phase-label {{ font-size: 10px; font-weight: bold; text-transform: uppercase; letter-spacing: 0.5px; display: inline-block; }}
  .phase-meta {{ float: right; font-size: 10px; color: #565f89; }}
  .phase-content {{ color: #a9b1d6; font-size: 12px; margin-top: 3px; max-height: 60px; overflow: hidden; white-space: pre-wrap; word-break: break-word; cursor: pointer; position: relative; }}
  .phase-content.expanded {{ max-height: 2000px; }}
  .phase.human {{ border-left-color: #7dcfff; }}
  .phase.human .phase-label {{ color: #7dcfff; }}
  .phase.thinking {{ border-left-color: #bb9af7; background: #24283b88; }}
  .phase.thinking .phase-label {{ color: #bb9af7; }}
  .phase.eval {{ border-left-color: #9ece6a; }}
  .phase.eval .phase-label {{ color: #9ece6a; }}
  .phase.apply {{ border-left-color: #e0af68; }}
  .phase.apply .phase-label {{ color: #e0af68; }}
  .phase.apply.agent {{ border-left-color: #ff9e64; }}
  .phase.apply.agent .phase-label {{ color: #ff9e64; }}
  .decision {{ display: inline-block; font-size: 9px; padding: 1px 5px; border-radius: 3px; margin-left: 6px; }}
  .decision.text_only {{ background: #9ece6a22; color: #9ece6a; }}
  .decision.tool_use {{ background: #e0af6822; color: #e0af68; }}

  .sentence {{ padding: 6px 10px; margin-bottom: 8px; color: #c0caf5; font-size: 12px; font-style: italic; border-bottom: 1px solid #2a2e42; padding-bottom: 8px; }}

  .domain-strip {{ display: flex; align-items: center; gap: 6px; padding: 4px 10px; flex-wrap: wrap; font-size: 11px; }}
  .domain-fact {{ display: inline-block; padding: 1px 6px; border-radius: 3px; font-size: 10px; }}
  .domain-fact.created {{ background: #9ece6a18; color: #9ece6a; }}
  .domain-fact.modified {{ background: #e0af6818; color: #e0af68; }}
  .domain-fact.read {{ background: #7dcfff18; color: #7dcfff; }}
  .domain-fact.cmd-ok {{ background: #9ece6a18; color: #9ece6a; }}
  .domain-fact.cmd-fail {{ background: #f7768e18; color: #f7768e; }}
  .domain-fact.search {{ background: #bb9af718; color: #bb9af7; }}
  .domain-fact.agent {{ background: #ff9e6418; color: #ff9e64; }}

  .footer-note {{ margin-top: 32px; color: #565f89; font-size: 11px; text-align: center; }}
</style>
</head>
<body>
<h1>The Metacircular Evaluator, Observed</h1>
<div class="subtitle">Session {escape(sid[:12])} &mdash; {len(turns)} turns. Each turn is one step of the coalgebra.</div>

<div class="legend">
  <div class="legend-item"><div class="legend-dot" style="background:#7dcfff"></div> human</div>
  <div class="legend-item"><div class="legend-dot" style="background:#bb9af7"></div> thinking</div>
  <div class="legend-item"><div class="legend-dot" style="background:#9ece6a"></div> eval</div>
  <div class="legend-item"><div class="legend-dot" style="background:#e0af68"></div> apply</div>
  <div class="legend-item"><div class="legend-dot" style="background:#ff9e64"></div> compound (agent)</div>
</div>

<div class="stats-bar">
  <div class="stat"><b>{len(turns)}</b> turns</div>
  <div class="stat"><b>{total_applies}</b> applies</div>
  <div class="stat"><b>{continued}</b> continued</div>
  <div class="stat"><b>{terminal}</b> terminated</div>
</div>

{turn_cards}

<div class="footer-note">
  Generated from <code>GET /api/sessions/{escape(sid[:12])}/turns</code>
  &middot; <a href="https://mitp-content-server.mit.edu/books/content/sectbyfn/books_pres_0/6515/sicp.zip/index.html" style="color:#7aa2f7">SICP</a>
</div>

<script>
document.addEventListener('click', e => {{
  const el = e.target.closest('.phase-content');
  if (el) el.classList.toggle('expanded');
  const header = e.target.closest('.turn-header');
  if (header) {{
    const body = header.nextElementSibling;
    const footer = body?.nextElementSibling;
    if (body?.classList.contains('turn-body')) {{
      const hidden = body.style.display === 'none';
      body.style.display = hidden ? 'block' : 'none';
      if (footer?.classList.contains('turn-footer')) footer.style.display = hidden ? 'flex' : 'none';
    }}
  }}
}});
</script>
</body>
</html>"""


def render_turn(turn, sentence_pattern):
    tn = turn["turn_number"]
    is_terminal = turn.get("is_terminal", True)
    badge_cls = "terminate" if is_terminal else "continue"
    badge_text = "terminate" if is_terminal else "continue"
    depth_indent = min(turn.get("scope_depth", 0) * 16, 48)

    phases = ""

    # Sentence one-liner
    if sentence_pattern:
        phases += f'<div class="sentence">{escape(sentence_pattern["summary"])}</div>\n'

    # Domain events strip
    applies = turn.get("applies", [])
    domain_facts = render_domain_strip(applies)
    if domain_facts:
        phases += domain_facts

    # Human
    human = turn.get("human")
    if human and human.get("content"):
        phases += f'''<div class="phase human">
          <span class="phase-label">human</span>
          <div class="phase-content">{escape(human["content"])}</div>
        </div>\n'''

    # Thinking
    thinking = turn.get("thinking")
    if thinking and thinking.get("summary"):
        summary = escape(thinking["summary"][:150])
        phases += f'''<div class="phase thinking">
          <span class="phase-label">thinking</span>
          <div class="phase-content">{summary}</div>
        </div>\n'''

    # Eval
    ev = turn.get("eval")
    if ev:
        decision = ev.get("decision", "text_only")
        decision_label = "text" if decision == "text_only" else "tool use"
        content = escape(ev.get("content", "(empty)"))
        phases += f'''<div class="phase eval">
          <span class="phase-label">eval</span>
          <span class="decision {decision}">{decision_label}</span>
          <div class="phase-content">{content}</div>
        </div>\n'''

    # Applies
    for apply in applies[:5]:
        cls = "apply agent" if apply.get("is_agent") else "apply"
        label = "apply \u00b7 compound" if apply.get("is_agent") else "apply"
        tool = escape(apply.get("tool_name", "?"))
        inp = escape(apply.get("input_summary", ""))
        out = escape(apply.get("output_summary", ""))
        outcome = apply.get("tool_outcome")
        outcome_badge = ""
        if outcome:
            otype = outcome.get("type", "")
            if otype == "FileCreated":
                outcome_badge = f'<span class="domain-fact created">+created</span>'
            elif otype == "FileModified":
                outcome_badge = f'<span class="domain-fact modified">~modified</span>'
            elif otype == "CommandExecuted":
                ok = outcome.get("succeeded", True)
                outcome_badge = f'<span class="domain-fact {"cmd-ok" if ok else "cmd-fail"}">{"ok" if ok else "failed"}</span>'

        phases += f'''<div class="phase {cls}">
          <span class="phase-label">{label}</span>
          <span class="phase-meta">{tool} {outcome_badge}</span>
          <div class="phase-content">{inp}</div>
          {f'<div class="phase-content" style="color:#565f89;font-size:11px">&rarr; {out[:200]}</div>' if out else ""}
        </div>\n'''

    if len(applies) > 5:
        phases += f'<div style="padding:4px 10px;color:#565f89;font-size:11px">... and {len(applies)-5} more applies</div>\n'

    # Footer
    env_size = turn.get("env_size", 0)
    env_delta = turn.get("env_delta", 0)
    stop = turn.get("stop_reason", "?")
    duration = turn.get("duration_ms")
    dur_str = f" \u00b7 {duration:.0f}ms" if duration else ""

    return f'''<div class="turn" style="margin-left:{depth_indent}px">
      <div class="turn-header">
        <div class="turn-left">
          <span class="turn-num">Turn {tn}</span>
        </div>
        <span class="badge {badge_cls}">{badge_text}</span>
      </div>
      <div class="turn-body">{phases}</div>
      <div class="turn-footer">
        <span>env: {env_size} messages {f'<span style="color:#9ece6a">(+{env_delta})</span>' if env_delta > 0 else ""}</span>
        <span style="color:{"#9ece6a" if is_terminal else "#e0af68"}">{stop} &rarr; {"TERMINATE" if is_terminal else "CONTINUE"} \u00b7 {len(applies)} applies{dur_str}</span>
      </div>
    </div>'''


def render_domain_strip(applies):
    if not applies:
        return ""
    facts = []
    for a in applies:
        outcome = a.get("tool_outcome")
        if not outcome:
            continue
        otype = outcome.get("type", "")
        if otype == "FileCreated":
            facts.append(f'<span class="domain-fact created">+{escape(outcome.get("path","")[-20:])}</span>')
        elif otype == "FileModified":
            facts.append(f'<span class="domain-fact modified">~{escape(outcome.get("path","")[-20:])}</span>')
        elif otype == "FileRead":
            facts.append(f'<span class="domain-fact read">{escape(outcome.get("path","")[-20:])}</span>')
        elif otype == "CommandExecuted":
            ok = outcome.get("succeeded", True)
            cmd = escape(outcome.get("command", "")[:30])
            facts.append(f'<span class="domain-fact {"cmd-ok" if ok else "cmd-fail"}">{cmd}</span>')
        elif otype == "SearchPerformed":
            facts.append(f'<span class="domain-fact search">{escape(outcome.get("pattern","")[:20])}</span>')
        elif otype == "SubAgentSpawned":
            facts.append(f'<span class="domain-fact agent">{escape(outcome.get("description","")[:20])}</span>')

    if not facts:
        return ""
    return f'<div class="domain-strip">{"".join(facts)}</div>\n'


if __name__ == "__main__":
    main()
