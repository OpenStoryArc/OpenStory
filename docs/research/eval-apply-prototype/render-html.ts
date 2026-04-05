// render-html.ts — The rich visualization.
//
// Pure function: SessionSummary → string (HTML)
// Shows the human, the thinking, the eval decision, the applies,
// the environment growth — the full life of each turn.

import type { SessionSummary, StructuralTurn, EvalDecision } from "./types.js";
import { buildSentence, type TurnSentence, type SubordinateClause } from "./sentence.js";
import { buildDomainTurn, type DomainTurn, type DomainEvent } from "./domain.js";

export function renderHtml(session: SessionSummary): string {
  const turns = session.turns.map((t, i) => renderTurnHtml(t, i)).join("\n");

  const toolBreakdown = Object.entries(session.toolCounts)
    .sort(([, a], [, b]) => b - a)
    .map(([name, count]) => `<span class="tool-badge">${esc(name)} <b>${count}</b></span>`)
    .join(" ");

  const continued = session.turns.filter(t => !t.isTerminal).length;
  const terminated = session.turns.filter(t => t.isTerminal).length;
  const thinkingTurns = session.turns.filter(t => t.thinking).length;

  // Compute env growth sparkline data
  const envData = session.turns.map(t => t.envSize);

  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Eval-Apply: ${esc(session.label.slice(0, 50))}</title>
<style>
  * { box-sizing: border-box; margin: 0; padding: 0; }
  body {
    background: #1a1b26;
    color: #c0caf5;
    font-family: 'JetBrains Mono', 'Fira Code', 'SF Mono', monospace;
    font-size: 13px;
    line-height: 1.6;
    padding: 24px;
    max-width: 960px;
    margin: 0 auto;
  }
  a { color: #7aa2f7; }
  h1 { color: #7aa2f7; font-size: 18px; margin-bottom: 4px; }
  .subtitle { color: #565f89; font-size: 12px; margin-bottom: 20px; }

  .stats-bar {
    display: flex; gap: 12px; flex-wrap: wrap;
    margin-bottom: 24px; padding: 12px 16px;
    background: #24283b; border-radius: 8px;
  }
  .stat { color: #a9b1d6; font-size: 12px; }
  .stat b { color: #c0caf5; }

  .legend {
    display: flex; gap: 14px; margin-bottom: 20px; flex-wrap: wrap;
  }
  .legend-item {
    display: flex; align-items: center; gap: 4px;
    font-size: 11px; color: #565f89;
  }
  .legend-dot { width: 10px; height: 10px; border-radius: 2px; }

  /* Turn card */
  .turn {
    margin-bottom: 8px;
    border-radius: 8px;
    background: #1f2335;
    overflow: hidden;
    border: 1px solid #2a2e42;
  }
  .turn:hover { border-color: #3b4261; }
  .turn-header {
    display: flex; justify-content: space-between; align-items: center;
    padding: 8px 14px;
    background: #24283b;
    cursor: pointer;
  }
  .turn-left { display: flex; align-items: center; gap: 10px; }
  .turn-num { color: #7aa2f7; font-weight: bold; font-size: 12px; }
  .turn-step { color: #565f89; font-size: 11px; }
  .badge {
    font-size: 9px; padding: 2px 7px; border-radius: 3px;
    font-weight: bold; text-transform: uppercase; letter-spacing: 0.5px;
  }
  .badge.terminate { background: #9ece6a18; color: #9ece6a; border: 1px solid #9ece6a33; }
  .badge.continue { background: #e0af6818; color: #e0af68; border: 1px solid #e0af6833; }

  .turn-body { padding: 10px 14px; }

  /* Phase blocks */
  .phase {
    padding: 6px 10px;
    margin: 4px 0;
    border-left: 3px solid;
    border-radius: 0 4px 4px 0;
    background: #24283b;
    position: relative;
  }
  .phase-label {
    font-size: 10px; font-weight: bold;
    text-transform: uppercase; letter-spacing: 0.5px;
    display: inline-block;
  }
  .phase-meta {
    float: right; font-size: 10px; color: #565f89;
  }
  .phase-content {
    color: #a9b1d6; font-size: 12px; margin-top: 3px;
    max-height: 60px; overflow: hidden;
    transition: max-height 0.4s ease;
    white-space: pre-wrap; word-break: break-word;
    cursor: pointer;
    position: relative;
  }
  .phase-content.expanded { max-height: 2000px; }
  .phase-content:not(.expanded)::after {
    content: '▼ click to expand';
    position: absolute; bottom: 0; right: 0;
    background: linear-gradient(to right, transparent, #24283b 30%);
    padding: 0 8px 0 24px;
    font-size: 10px; color: #565f89;
  }
  .phase-content.expanded::after {
    content: '▲ collapse';
    position: relative; display: block;
    text-align: right;
    font-size: 10px; color: #565f89;
    margin-top: 4px;
  }
  .phase-annotation {
    color: #565f89; font-size: 10px; font-style: italic; margin-top: 2px;
  }

  /* Human phase */
  .phase.human { border-left-color: #7dcfff; }
  .phase.human .phase-label { color: #7dcfff; }

  /* Thinking phase */
  .phase.thinking { border-left-color: #bb9af7; background: #24283b88; }
  .phase.thinking .phase-label { color: #bb9af7; }

  /* Eval phase */
  .phase.eval { border-left-color: #9ece6a; }
  .phase.eval .phase-label { color: #9ece6a; }

  /* Apply phase */
  .phase.apply { border-left-color: #e0af68; }
  .phase.apply .phase-label { color: #e0af68; }
  .phase.apply.agent { border-left-color: #ff9e64; }
  .phase.apply.agent .phase-label { color: #ff9e64; }
  .phase.apply.error { border-left-color: #f7768e; }

  /* Compact */
  .phase.compact { border-left-color: #f7768e; }
  .phase.compact .phase-label { color: #f7768e; }

  /* Decision badge */
  .decision {
    display: inline-block; font-size: 9px; padding: 1px 5px;
    border-radius: 3px; margin-left: 6px;
  }
  .decision.text_only { background: #9ece6a22; color: #9ece6a; }
  .decision.tool_use { background: #e0af6822; color: #e0af68; }
  .decision.text_and_tool_use { background: #7dcfff22; color: #7dcfff; }

  /* Footer */
  .turn-footer {
    display: flex; justify-content: space-between;
    padding: 4px 14px 8px;
    font-size: 11px; color: #565f89;
  }
  .env-bar {
    display: inline-block; height: 4px; border-radius: 2px;
    background: #3b4261; margin-left: 6px; vertical-align: middle;
  }
  .env-fill {
    display: inline-block; height: 4px; border-radius: 2px;
    background: #7aa2f7;
  }

  /* Collapsible tool list */
  .apply-collapsed {
    margin: 4px 0; cursor: pointer;
  }
  .apply-collapsed-summary {
    padding: 6px 10px;
    border-left: 3px solid #e0af68;
    border-radius: 0 4px 4px 0;
    background: #24283b;
    font-size: 11px; color: #a9b1d6;
    transition: background 0.2s;
  }
  .apply-collapsed-summary:hover { background: #2a3050; }
  .apply-collapsed .tool-name { color: #e0af68; }
  .apply-collapsed-detail {
    display: none;
    margin-left: 4px;
  }
  .apply-collapsed.open .apply-collapsed-summary {
    color: #565f89;
  }
  .apply-collapsed.open .apply-collapsed-summary::before {
    content: '▼ ';
  }
  .apply-collapsed:not(.open) .apply-collapsed-summary::before {
    content: '';
  }
  .apply-collapsed.open .apply-collapsed-detail {
    display: block;
  }

  /* Summary section */
  .summary {
    margin-top: 32px; padding: 16px;
    background: #24283b; border-radius: 8px;
  }
  .summary h2 { color: #7aa2f7; font-size: 15px; margin-bottom: 12px; }
  .summary-grid {
    display: grid; grid-template-columns: 1fr 1fr 1fr;
    gap: 8px;
  }
  .tool-badge {
    display: inline-block; padding: 2px 8px;
    background: #1a1b26; border-radius: 4px;
    font-size: 11px; margin: 2px;
  }

  /* Sentence one-liner */
  .sentence {
    padding: 6px 10px;
    margin-bottom: 8px;
    color: #c0caf5;
    font-size: 12px;
    font-style: italic;
    border-bottom: 1px solid #2a2e42;
    padding-bottom: 8px;
  }

  /* Domain events strip */
  .domain-strip {
    display: flex; align-items: center; gap: 6px;
    padding: 4px 10px; flex-wrap: wrap;
    font-size: 11px;
  }
  .domain-actor { font-size: 14px; }
  .domain-fact {
    display: inline-block; padding: 1px 6px;
    border-radius: 3px; font-size: 10px;
  }
  .domain-fact.created { background: #9ece6a18; color: #9ece6a; }
  .domain-fact.modified { background: #e0af6818; color: #e0af68; }
  .domain-fact.read { background: #7dcfff18; color: #7dcfff; }
  .domain-fact.cmd-ok { background: #9ece6a18; color: #9ece6a; }
  .domain-fact.cmd-fail { background: #f7768e18; color: #f7768e; }
  .domain-fact.search { background: #bb9af718; color: #bb9af7; }
  .domain-fact.agent { background: #ff9e6418; color: #ff9e64; }

  .domain-detail {
    margin: 2px 10px 6px; cursor: pointer; font-size: 11px;
  }
  .domain-detail-hint { color: #565f89; font-size: 10px; }
  .domain-detail-hint:hover { color: #7aa2f7; }
  .domain-detail-body {
    display: none; padding: 6px 8px;
    background: #1a1b26; border-radius: 4px; margin-top: 4px;
  }
  .domain-detail.open .domain-detail-body { display: block; }
  .domain-detail.open .domain-detail-hint { display: none; }
  .domain-event-row {
    padding: 1px 0; font-size: 11px; color: #a9b1d6;
  }
  .domain-path { color: #7aa2f7; }
  .domain-event-row code {
    background: #24283b; padding: 0 4px; border-radius: 2px;
    font-size: 10px;
  }

  /* Sentence diagram */
  .diagram {
    margin: 4px 0 8px;
    cursor: pointer;
    font-size: 11px;
  }
  .diagram-hint {
    color: #565f89; font-size: 10px;
    padding: 2px 10px;
  }
  .diagram-hint:hover { color: #7aa2f7; }
  .diagram-body {
    display: none;
    padding: 8px 12px;
    background: #1a1b26;
    border-radius: 4px;
    font-family: 'JetBrains Mono', monospace;
    margin: 4px 10px;
  }
  .diagram.open .diagram-body { display: block; }
  .diagram.open .diagram-hint { display: none; }

  .diagram-main {
    margin-bottom: 4px;
  }
  .diagram-subject { color: #7aa2f7; font-weight: bold; }
  .diagram-line { color: #3b4261; }
  .diagram-verb { color: #9ece6a; font-weight: bold; }
  .diagram-object { color: #c0caf5; }
  .diagram-count { color: #565f89; font-size: 10px; }
  .diagram-clause {
    padding-left: 20px;
    margin: 2px 0;
  }
  .diagram-branch { color: #3b4261; }
  .diagram-predicate {
    margin-top: 4px;
    padding-left: 20px;
    color: #9ece6a;
  }

  .footer-note {
    margin-top: 32px; color: #565f89; font-size: 11px; text-align: center;
  }
</style>
</head>
<body>

<h1>The Metacircular Evaluator, Observed</h1>
<div class="subtitle">
  Structural view — each turn is one step of the coalgebra.
  Human speaks, model thinks, eval decides, tools apply.
</div>

<div class="legend">
  <div class="legend-item"><div class="legend-dot" style="background:#7dcfff"></div> human</div>
  <div class="legend-item"><div class="legend-dot" style="background:#bb9af7"></div> thinking</div>
  <div class="legend-item"><div class="legend-dot" style="background:#9ece6a"></div> eval</div>
  <div class="legend-item"><div class="legend-dot" style="background:#e0af68"></div> apply</div>
  <div class="legend-item"><div class="legend-dot" style="background:#ff9e64"></div> compound (agent)</div>
  <div class="legend-item"><div class="legend-dot" style="background:#f7768e"></div> compaction (GC)</div>
</div>

<div class="stats-bar">
  <div class="stat"><b>${esc(session.model)}</b></div>
  <div class="stat"><b>${session.turns.length}</b> turns</div>
  <div class="stat"><b>${session.totalApplies}</b> applies</div>
  <div class="stat"><b>${continued}</b> continued</div>
  <div class="stat"><b>${terminated}</b> terminated</div>
  <div class="stat">${thinkingTurns} turns with thinking</div>
  <div class="stat">env: <b>${session.envGrowth[0]}</b> → <b>${session.envGrowth[1]}</b></div>
</div>

${turns}

<div class="summary">
  <h2>Session Summary</h2>
  <div class="summary-grid">
    <div class="stat">Evals: <b>${session.totalEvals}</b></div>
    <div class="stat">Applies: <b>${session.totalApplies}</b></div>
    <div class="stat">Thinking: <b>${session.totalThinkingTokens}</b> tokens</div>
    <div class="stat">Max depth: <b>${session.maxScopeDepth}</b></div>
    <div class="stat">Compactions: <b>${session.compactionCount}</b></div>
    <div class="stat">Env growth: <b>${session.envGrowth[1] - session.envGrowth[0]}</b> messages</div>
  </div>
  <div style="margin-top: 12px">
    <div class="stat">Tools: ${toolBreakdown}</div>
  </div>
</div>

<div class="footer-note">
  <a href="https://mitp-content-server.mit.edu/books/content/sectbyfn/books_pres_0/6515/sicp.zip/index.html">SICP</a>
  · Church → McCarthy → Sussman → the agent loop
  · Generated by the eval-apply detector prototype
</div>

<script>
// Click to expand/collapse phase content
document.addEventListener('click', (e) => {
  const el = e.target.closest('.phase-content');
  if (el) el.classList.toggle('expanded');
});

// Click turn header to collapse/expand the body
document.addEventListener('click', (e) => {
  const header = e.target.closest('.turn-header');
  if (header) {
    const body = header.nextElementSibling;
    const footer = body?.nextElementSibling;
    if (body?.classList.contains('turn-body')) {
      const hidden = body.style.display === 'none';
      body.style.display = hidden ? 'block' : 'none';
      if (footer?.classList.contains('turn-footer')) {
        footer.style.display = hidden ? 'flex' : 'none';
      }
    }
  }
});
</script>

</body>
</html>`;
}

function renderTurnHtml(turn: StructuralTurn, index: number): string {
  const cls = turn.isTerminal ? "terminate" : "continue";
  const badge = turn.isTerminal
    ? `<span class="badge terminate">terminate</span>`
    : `<span class="badge continue">continue</span>`;

  // Build the sentence
  const sentence = buildSentence(turn);

  let phases = "";

  // Sentence one-liner at the top
  phases += `
    <div class="sentence">
      ${esc(sentence.oneLiner)}
    </div>`;

  // Sentence diagram (collapsible)
  phases += renderDiagram(sentence);

  // Domain events (the facts, collapsible)
  const domain = buildDomainTurn(turn);
  phases += renderDomainEvents(domain);

  // Human
  if (turn.human) {
    phases += `
      <div class="phase human">
        <span class="phase-label">human</span>
        <div class="phase-content">${esc(turn.human.content)}</div>
      </div>`;
  }

  // Thinking
  if (turn.thinking && turn.thinking.tokens > 0) {
    const summary = turn.thinking.summary
      ? esc(turn.thinking.summary.slice(0, 150))
      : "(redacted)";
    phases += `
      <div class="phase thinking">
        <span class="phase-label">thinking</span>
        <span class="phase-meta">${turn.thinking.tokens} tokens</span>
        <div class="phase-content">${summary}</div>
        <div class="phase-annotation">reasoning before responding — invisible to the user</div>
      </div>`;
  }

  // Eval
  if (turn.eval) {
    const decisionLabel = turn.eval.decision === "text_only" ? "text"
      : turn.eval.decision === "tool_use" ? "tool use"
      : "text + tool use";
    const decisionCls = turn.eval.decision;
    const content = turn.eval.content || "(empty)";
    phases += `
      <div class="phase eval">
        <span class="phase-label">eval</span>
        <span class="decision ${decisionCls}">${decisionLabel}</span>
        <span class="phase-meta">${turn.eval.tokens > 0 ? turn.eval.tokens + " tokens" : ""}</span>
        <div class="phase-content">${esc(content)}</div>
      </div>`;
  }

  // Applies — all rendered, but collapse after first 2 if many
  if (turn.applies.length <= 3) {
    for (const apply of turn.applies) {
      phases += renderApply(apply);
    }
  } else {
    // Show first 2 always visible
    for (const apply of turn.applies.slice(0, 2)) {
      phases += renderApply(apply);
    }
    // Rest hidden behind a clickable group
    const rest = turn.applies.slice(2);
    const grouped: Record<string, number> = {};
    for (const a of rest) {
      grouped[a.toolName] = (grouped[a.toolName] ?? 0) + 1;
    }
    const summary = Object.entries(grouped)
      .map(([name, count]) => `<span class="tool-name">${esc(name)}</span> ×${count}`)
      .join(", ");
    const hiddenApplies = rest.map(a => renderApply(a)).join("\n");
    phases += `
      <div class="apply-collapsed" onclick="this.classList.toggle('open')">
        <div class="apply-collapsed-summary">
          ▶ ... and ${rest.length} more: ${summary}
        </div>
        <div class="apply-collapsed-detail">
          ${hiddenApplies}
        </div>
      </div>`;
  }

  // Env bar — proportional width based on max env in session
  const maxEnv = 600; // rough max for scaling
  const envPct = Math.min(100, Math.round((turn.envSize / maxEnv) * 100));
  const depthIndent = Math.min(turn.scopeDepth * 16, 48);

  return `
    <div class="turn" style="margin-left: ${depthIndent}px">
      <div class="turn-header">
        <div class="turn-left">
          <span class="turn-num">Turn ${turn.turnNumber}</span>
          <span class="turn-step">coalgebra step ${turn.turnNumber}${turn.scopeDepth > 0 ? ` · depth ${turn.scopeDepth}` : ""}</span>
        </div>
        ${badge}
      </div>
      <div class="turn-body">
        ${phases}
      </div>
      <div class="turn-footer">
        <span>
          env: ${turn.envSize} messages
          ${turn.envDelta > 0 ? `<span style="color: #9ece6a">(+${turn.envDelta})</span>` : ""}
          <span class="env-bar" style="width: 80px"><span class="env-fill" style="width: ${envPct}%"></span></span>
        </span>
        <span style="color: ${turn.isTerminal ? '#9ece6a' : '#e0af68'}">
          ${turn.stopReason} → ${turn.isTerminal ? 'TERMINATE' : 'CONTINUE'}
          ${turn.applies.length > 0 ? ` · ${turn.applies.length} tool call${turn.applies.length > 1 ? 's' : ''}` : ''}
          ${turn.eval && turn.eval.content.includes('[') && turn.eval.content.includes('eval cycles') ? ` · ${turn.eval.content.match(/\[(\d+) eval/)?.[1] ?? ''} eval cycles` : ''}
        </span>
      </div>
    </div>`;
}

function renderDiagram(sentence: TurnSentence): string {
  const roleColors: Record<string, string> = {
    preparatory: "#7dcfff",
    creative: "#9ece6a",
    verificatory: "#e0af68",
    delegatory: "#bb9af7",
    interactive: "#565f89",
  };

  let clauses = "";
  for (const sub of sentence.subordinates) {
    const color = roleColors[sub.role] ?? "#565f89";
    clauses += `
      <div class="diagram-clause">
        <span class="diagram-branch">├──</span>
        <span style="color: ${color}">${esc(sub.verb)}</span>
        <span class="diagram-object">${esc(sub.object)}</span>
        <span class="diagram-count">(${sub.toolCalls})</span>
      </div>`;
  }

  if (sentence.adverbial) {
    clauses += `
      <div class="diagram-clause">
        <span class="diagram-branch">└──</span>
        <span style="color: #f7768e">because</span>
        <span class="diagram-object">${esc(sentence.adverbial)}</span>
      </div>`;
  }

  if (!clauses) return "";

  return `
    <div class="diagram" onclick="this.classList.toggle('open')">
      <div class="diagram-hint">▶ diagram</div>
      <div class="diagram-body">
        <div class="diagram-main">
          <span class="diagram-subject">${esc(sentence.subject)}</span>
          <span class="diagram-line">────</span>
          <span class="diagram-verb">${esc(sentence.verb)}</span>
          <span class="diagram-line">────</span>
          <span class="diagram-object">${esc(sentence.object)}</span>
        </div>
        <div class="diagram-clauses">
          ${clauses}
        </div>
        <div class="diagram-predicate">
          → ${esc(sentence.predicate)}
        </div>
      </div>
    </div>`;
}

function renderDomainEvents(domain: DomainTurn): string {
  if (domain.events.length === 0) return "";

  const agg = domain.aggregate;
  const facts: string[] = [];

  if (agg.filesCreated.length > 0)
    facts.push(`<span class="domain-fact created">+${agg.filesCreated.length} created</span>`);
  if (agg.filesModified.length > 0)
    facts.push(`<span class="domain-fact modified">~${agg.filesModified.length} modified</span>`);
  if (agg.filesRead.length > 0)
    facts.push(`<span class="domain-fact read">${agg.filesRead.length} read</span>`);
  if (agg.commandsSucceeded > 0)
    facts.push(`<span class="domain-fact cmd-ok">${agg.commandsSucceeded} cmd ok</span>`);
  if (agg.commandsFailed > 0)
    facts.push(`<span class="domain-fact cmd-fail">${agg.commandsFailed} cmd failed</span>`);
  if (agg.searchesPerformed > 0)
    facts.push(`<span class="domain-fact search">${agg.searchesPerformed} searches</span>`);
  if (agg.subAgentsSpawned > 0)
    facts.push(`<span class="domain-fact agent">${agg.subAgentsSpawned} sub-agents</span>`);

  const eventRows = domain.events.map(e => {
    const icon = eventIcon(e);
    const desc = eventDescription(e);
    return `<div class="domain-event-row">${icon} ${desc}</div>`;
  }).join("\n");

  return `
    <div class="domain-strip">
      <span class="domain-actor">${domain.initiator === "human" ? "👤" : "🤖"}</span>
      ${facts.join(" ")}
    </div>
    <div class="domain-detail" onclick="this.classList.toggle('open')">
      <div class="domain-detail-hint">▶ ${domain.events.length} domain events (facts)</div>
      <div class="domain-detail-body">
        ${eventRows}
      </div>
    </div>`;
}

function eventIcon(e: DomainEvent): string {
  switch (e.type) {
    case "FileCreated": return '<span style="color:#9ece6a">+</span>';
    case "FileModified": return '<span style="color:#e0af68">~</span>';
    case "FileRead": return '<span style="color:#7dcfff">⊳</span>';
    case "FileWriteFailed": case "FileReadFailed": return '<span style="color:#f7768e">✗</span>';
    case "SearchPerformed": return '<span style="color:#bb9af7">⌕</span>';
    case "CommandExecuted": return (e as any).succeeded ? '<span style="color:#9ece6a">$</span>' : '<span style="color:#f7768e">$</span>';
    case "SubAgentSpawned": return '<span style="color:#ff9e64">⊕</span>';
    case "ResponseDelivered": return '<span style="color:#7aa2f7">→</span>';
    default: return "·";
  }
}

function eventDescription(e: DomainEvent): string {
  switch (e.type) {
    case "FileCreated": return `created <span class="domain-path">${esc((e as any).path)}</span>`;
    case "FileModified": return `modified <span class="domain-path">${esc((e as any).path)}</span>`;
    case "FileRead": return `read <span class="domain-path">${esc((e as any).path)}</span>`;
    case "FileWriteFailed": return `write failed: <span class="domain-path">${esc((e as any).path)}</span>`;
    case "FileReadFailed": return `read failed: <span class="domain-path">${esc((e as any).path)}</span>`;
    case "SearchPerformed": return `searched ${esc((e as any).source)}: <span class="domain-path">${esc((e as any).pattern).slice(0, 60)}</span>`;
    case "CommandExecuted": {
      const cmd = esc((e as any).command).slice(0, 80);
      const status = (e as any).succeeded ? "" : ' <span style="color:#f7768e">(failed)</span>';
      return `ran <code>${cmd}</code>${status}`;
    }
    case "SubAgentSpawned": return `spawned sub-agent: ${esc((e as any).description).slice(0, 60)}`;
    case "ResponseDelivered": return `delivered response (${(e as any).tokens} tokens)`;
    default: return esc(e.type);
  }
}

function renderApply(apply: { toolName: string; inputSummary: string; outputSummary: string; isAgent: boolean; isError: boolean }): string {
  const cls = apply.isAgent ? "apply agent" : apply.isError ? "apply error" : "apply";
  const label = apply.isAgent ? "apply · compound" : "apply";
  return `
    <div class="phase ${cls}">
      <span class="phase-label">${label}</span>
      <span class="phase-meta">${esc(apply.toolName)}</span>
      <div class="phase-content">${esc(apply.inputSummary)}</div>
      ${apply.outputSummary ? `<div class="phase-content" style="color: #565f89; font-size: 11px;">→ ${esc(apply.outputSummary)}</div>` : ""}
      ${apply.isAgent ? `<div class="phase-annotation">nested eval-apply loop with fresh scope (SICP §4.1.3)</div>` : ""}
    </div>`;
}

function esc(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
