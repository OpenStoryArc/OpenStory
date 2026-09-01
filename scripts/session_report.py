"""Single-session archetype report — self-contained, cited, validatable HTML.

Composes everything we built into ONE portable file for ONE session:
  • metric_trace.session_reading / trace_json / event_detail  → the reading + the
    procedural trace + the exact diffs
  • cited_schema.assessment_for                               → the Toulmin chain
  • cited_schema schemas                                      → per-field x-citations
    and a validation pass (the report says ✓ conforms, in the page)

All data is embedded — open the file anywhere, offline. Sovereign by construction.

    python3 scripts/session_report.py <SID>                  # → /tmp/session-<id>.html
    python3 scripts/session_report.py <SID> --out report.html
    python3 scripts/session_report.py --self                 # most-recent session
    python3 scripts/session_report.py --test
"""

import argparse
import json
import sys
from pathlib import Path

import profile_dimensions as pd
import metric_trace as mt
import cited_schema as cs


def build_report_data(api: str, sid: str) -> dict:
    """Compose the cited single-session structure from live events."""
    sess = next((s for s in (pd._get(api, "/api/sessions") or {}).get("sessions", [])
                 if s.get("session_id") == sid), {"session_id": sid})
    records = mt.records_for(api, sid)
    steps = mt.steps_from_records(records)
    reading = mt.session_reading(steps)
    trace = mt.trace_json(api, sid)
    assessment = cs.assessment_for(api, sid)

    # Validate both structures against their cited schemas — status shown in-page.
    reading_errs = cs.validate(reading, cs.reading_schema())
    assess_errs = cs.validate(assessment, cs.assessment_schema())

    # Per-signal citations, straight from the schema's x-citation.
    sigprops = cs.reading_schema()["properties"]["signals"]["properties"]
    citations = {name: sigprops[name]["x-citation"] for name in sigprops}

    # Embed event details (the exact diff/command+output) for every shown event.
    seqs = {c["seq"] for d in trace["derivations"] for c in d["contributors"] if "seq" in c}
    events = {str(s): mt.event_detail(records, s) for s in sorted(seqs)}

    reg = cs.load_registry()
    theories = {tid: {"scholar": r["scholar"], "year": r["year"],
                      "primary_work": r["primary_work"], "primary_source": r["primary_source"]}
                for tid, r in reg.items()}

    return {
        "session": {"sid": sid,
                    "project": _short(sess.get("project_name") or sess.get("project_id") or "?"),
                    "label": (sess.get("label") or "").strip()[:90],
                    "events": sess.get("event_count", 0),
                    "agent": sess.get("origin_agent", "?")},
        "reading": reading, "reading_valid": reading_errs == [], "reading_errs": reading_errs,
        "assessment": assessment, "assessment_valid": assess_errs == [], "assessment_errs": assess_errs,
        "trace": trace, "events": events, "citations": citations, "theories": theories,
    }


def _short(p: str) -> str:
    return p.rstrip("-").split("-")[-1] or p if p else "?"


# ── Self-contained HTML (data embedded; no fetching) ──────────────────────────
def render_html(data: dict) -> str:
    payload = json.dumps(data, ensure_ascii=False)
    return _TEMPLATE.replace("/*__DATA__*/", payload)


_TEMPLATE = r"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1"/>
<title>Open Story · Session Report</title>
<style>
:root{--bg:#0b0d10;--panel:#12161c;--line:#222a33;--ink:#e8edf2;--muted:#7d8a99;
 --accent:#5ad1c5;--warn:#f2b15a;--bad:#e0667a;--good:#7ee0a8;
 --mono:ui-monospace,"SF Mono","JetBrains Mono",Menlo,Consolas,monospace;}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--ink);font-family:var(--mono);line-height:1.55}
.wrap{max-width:860px;margin:0 auto;padding:40px 24px 90px}a{color:var(--accent);text-decoration:none}a:hover{text-decoration:underline}
header{border-bottom:1px solid var(--line);padding-bottom:16px}
.brand{letter-spacing:.18em;font-size:12px;color:var(--muted);text-transform:uppercase}
.sub{color:var(--muted);font-size:13px;margin-top:10px}
.badges{margin-top:12px;display:flex;gap:8px;flex-wrap:wrap}
.badge{font-size:11px;border-radius:999px;padding:3px 10px;border:1px solid}
.badge.ok{color:var(--good);border-color:var(--good)}.badge.bad{color:var(--bad);border-color:var(--bad)}
.badge.local{color:var(--accent);border-color:var(--accent)}
h2{font-size:11px;letter-spacing:.16em;text-transform:uppercase;color:var(--muted);font-weight:600;margin:0 0 14px}
.panel{background:var(--panel);border:1px solid var(--line);border-radius:14px;padding:22px;margin-top:22px}
.claim{font-size:30px;font-weight:700;margin:4px 0}
.gauge{position:relative;height:42px;margin:10px 0 4px}
.gt{position:absolute;top:18px;left:0;right:0;height:6px;border-radius:3px;background:linear-gradient(90deg,#3a2a18,#1a2a2e,#103a36);border:1px solid var(--line)}
.gm{position:absolute;top:11px;left:50%;width:1px;height:20px;background:var(--muted);opacity:.5}
.gd{position:absolute;top:12px;width:15px;height:15px;border-radius:50%;background:var(--warn);box-shadow:0 0 12px var(--warn);transform:translateX(-50%)}
.gends{display:flex;justify-content:space-between;font-size:12px;color:var(--muted)}
.chain .row{margin:11px 0}.chain .k{font-size:10px;letter-spacing:.14em;text-transform:uppercase;color:var(--muted)}
.chain .t{font-size:13.5px;margin-top:3px}
.bdg{display:inline-block;font-size:10px;text-transform:uppercase;letter-spacing:.05em;border-radius:999px;padding:2px 8px;border:1px solid}
.bdg.validated,.bdg\.validated-frame{color:var(--good);border-color:var(--good)}
.bdg.analogical{color:var(--warn);border-color:var(--warn)}
.bdg.metaphorical,.bdg.none{color:var(--bad);border-color:var(--bad)}
.sig{border-top:1px solid var(--line);padding:10px 0}.sig:first-child{border-top:0}
.sighead{display:flex;align-items:center;gap:10px;cursor:pointer}
.sighead .nm{font-size:13.5px}.sighead .val{margin-left:auto;color:var(--muted)}
.pl{font-size:10px;text-transform:uppercase;letter-spacing:.05em}.pl.deliberate{color:var(--accent)}.pl.spontaneous{color:var(--warn)}
.cit{display:none;margin:8px 0 2px 4px;border-left:2px solid var(--line);padding-left:12px;font-size:12px;color:var(--muted)}
.cit.open{display:block}.cit .w{color:var(--ink)}.cit .flaw{color:var(--warn)}
.deriv{border-top:1px solid var(--line);padding:12px 0}.deriv:first-child{border-top:0}
.dhead{display:flex;gap:10px;align-items:baseline}.dhead .nm{font-size:13.5px}.dhead .v{margin-left:auto;color:var(--muted);font-size:13px}.dhead .v b{color:var(--ink)}
.dproc{font-size:12px;color:var(--muted);margin:3px 0 6px}
details summary{cursor:pointer;font-size:12px;color:var(--accent)}
.evlist{margin:8px 0;border-left:2px solid var(--line);padding-left:12px}
.ev{font-size:11.5px;color:var(--muted);margin:3px 0;font-variant-numeric:tabular-nums}
.ev .sq{color:var(--ink)}.ev .eid{color:#4b5563}.ev .yes{color:var(--accent)}.ev .no{color:var(--warn)}
.ev .exp{color:var(--accent);cursor:pointer;margin-left:6px;font-size:11px}
.chg{display:none;margin:4px 0 8px 14px;border-left:2px solid var(--line);padding-left:10px}.chg.open{display:block}
.diff{font-size:11px;line-height:1.45;overflow-x:auto}.dl{white-space:pre}
.dl.add{color:var(--good);background:rgba(90,209,150,.06)}.dl.del{color:var(--bad);background:rgba(224,102,122,.06)}.dl.ctx{color:var(--muted)}.dl.hunk{color:#6a86c9}
.chg pre{font-size:11px;white-space:pre-wrap;word-break:break-word;background:#0a0c0f;border:1px solid var(--line);border-radius:6px;padding:8px;max-height:260px;overflow:auto;color:var(--ink)}
.chg pre.cmd{color:var(--accent)}.stat{font-size:11px;color:var(--muted)}
.dlossy{font-size:12px;color:var(--warn);margin-top:6px}.foot{color:var(--muted);font-size:12px;margin-top:8px}
</style></head><body><div class="wrap">
<header>
 <span class="brand">Open Story · Session Report</span>
 <div id="hdr"></div>
 <div class="badges" id="badges"></div>
</header>
<div id="body"></div>
</div>
<script>
const DATA = /*__DATA__*/;
const $=s=>document.querySelector(s);
const esc=s=>(s||"").replace(/[&<>]/g,c=>({"&":"&amp;","<":"&lt;",">":"&gt;"}[c]));
const SHORT={read_before_edit:"reads first",test_cadence:"runs tests",plan_density:"plans",search_fanout:"searches",context_switch:"switches files",churn:"re-edits",seq_entropy:"varied tools"};
function cite(c){
 const root=c.primary_source?`<a href="${c.primary_source}" target="_blank">${esc(c.primary_source)}</a>`:'⚠ UNCITED (heuristic / metaphorical)';
 return `<div><span class="w">warrant</span> — ${esc(c.warrant)}</div>
  <div><span class="w">strength</span> — <span class="bdg ${c.warrant_strength}">${c.warrant_strength}</span> · pole ${c.pole||'—'}</div>
  <div><span class="w">root</span> — ${root}</div>${c.flaw?`<div class="flaw">⚠ ${esc(c.flaw)}</div>`:''}`;
}
function detail(box,d){
 if(!d){box.innerHTML='<div class="stat">no detail</div>';return;}
 if(d.kind==="diff"||d.kind==="write"){const s=d.stats||{};
  box.innerHTML=`<div class="stat">${d.tool} ${esc(d.target)} · <span style="color:var(--good)">+${s.added||0}</span> / <span style="color:var(--bad)">−${s.removed||0}</span></div>`
   +`<div class="diff">${(d.diff||[]).map(l=>`<div class="dl ${l.k}">${esc(l.text)||" "}</div>`).join("")}</div>`+(d.truncated?'<div class="stat">… truncated</div>':'');
 }else if(d.kind==="bash"){box.innerHTML=`<pre class="cmd">$ ${esc(d.command)}</pre>`+(d.output?`<pre>${esc(d.output)}</pre>`:'');}
 else{box.innerHTML=`<pre>${esc(d.input||'')}</pre>`+(d.output?`<pre>${esc(d.output)}</pre>`:'');}
}
function evRow(d,c){
 if(c.bigram!==undefined)return `<div class="ev">${c.bigram} ×${c.n}</div>`;
 const id8=c.id?`<span class="eid">${c.id.slice(0,8)}</span>`:'';
 let label=c.rel||c.cmd||c.tool||'';
 if(d.signal==="read_before_edit")label=`<span class="${c.read_first?'yes':'no'}">${c.read_first?'↑read-first':'✗not-read'}</span> ${c.tool} ${c.rel}`;
 else if(d.signal==="context_switch")label=c.rel;
 else if(d.signal==="churn")label=`re-edit ${c.rel} <span class="eid">(first @seq ${c.prior_seq})</span>`;
 else if(d.signal==="test_cadence")label=`<span class="eid">${(c.cmd||'').slice(0,56)}</span>`;
 const k=`${d.signal}-${c.seq}`, has=DATA.events[c.seq]!==undefined;
 const exp=has?`<span class="exp" data-k="${k}" data-seq="${c.seq}">▸ change</span>`:'';
 return `<div class="ev"><span class="sq">seq ${c.seq}</span> ${id8} · ${label}${exp}</div>`+(has?`<div class="chg" id="chg-${k}"></div>`:'');
}
function render(){
 const s=DATA.session, r=DATA.reading, a=DATA.assessment;
 $("#hdr").innerHTML=`<div class="sub"><b>${esc(s.label||s.sid.slice(0,8))}</b><br>${s.project} · ${s.events} events · agent ${s.agent} · <code>${s.sid.slice(0,8)}</code></div>`;
 $("#badges").innerHTML=`<span class="badge local">local-only</span>`
  +`<span class="badge ${DATA.reading_valid?'ok':'bad'}">${DATA.reading_valid?'✓':'✗'} session-reading.schema</span>`
  +`<span class="badge ${DATA.assessment_valid?'ok':'bad'}">${DATA.assessment_valid?'✓':'✗'} assessment.schema</span>`;
 const dot=50+r.net_lean*50;
 const back=a.backing[0], bt=DATA.theories[back.theory_id]||{};
 let body=`
 <div class="panel">
   <h2>The verdict · this session</h2>
   <div class="claim">${esc(a.claim)}</div>
   <div class="sub">net lean ${r.net_lean>=0?'+':''}${r.net_lean.toFixed(2)} · deliberate ${r.deliberate_index.toFixed(2)} vs spontaneous ${r.spontaneous_index.toFixed(2)}</div>
   <div class="gauge"><div class="gt"></div><div class="gm"></div><div class="gd" style="left:${dot}%"></div></div>
   <div class="gends"><span>spontaneous (associative)</span><span>deliberate (controlled)</span></div>
 </div>

 <div class="panel">
   <h2>The argument · Toulmin chain <span class="badge ok" style="font-size:10px">✓ assessment.schema</span></h2>
   <div class="chain">
     <div class="row"><div class="k">Claim</div><div class="t">${esc(a.claim)}</div></div>
     <div class="row"><div class="k">Grounds</div><div class="t">${r.n_events} tool_call events · net_lean ${a.grounds.net_lean.toFixed(2)} (downward provenance → the trace below)</div></div>
     <div class="row"><div class="k">Warrant</div><div class="t">${esc(a.warrant)}</div></div>
     <div class="row"><div class="k">Backing</div><div class="t">${bt.primary_source?`<a href="${bt.primary_source}" target="_blank">${bt.scholar} (${bt.year})</a>, ${esc(bt.primary_work)}`:back.theory_id} <span class="bdg ${back.warrant_strength}">${back.warrant_strength}</span></div></div>
     <div class="row"><div class="k">Qualifier</div><div class="t">confidence ${a.qualifier.confidence} · <span class="bdg ${a.qualifier.warrant_strength}">${a.qualifier.warrant_strength}</span></div></div>
     <div class="row"><div class="k">Rebuttal</div><div class="t">${esc(a.rebuttal)}</div></div>
   </div>
 </div>

 <div class="panel">
   <h2>Signals · each with its citation</h2>
   <div class="sub" style="margin-top:-6px">Click a signal to see its research root and warrant strength (from the cited schema).</div>
   <div style="margin-top:10px">`;
 for(const [name,v] of Object.entries(r.signals)){
   const c=DATA.citations[name]||{};
   body+=`<div class="sig">
     <div class="sighead" data-cit="${name}"><span class="nm">${SHORT[name]||name}</span>
       <span class="pl ${c.pole||''}">${c.pole||''}</span>
       <span class="bdg ${c.warrant_strength}">${c.warrant_strength}</span>
       <span class="val">${v.toFixed(2)}</span></div>
     <div class="cit" id="cit-${name}">${cite(c)}</div></div>`;
 }
 body+=`</div></div>

 <div class="panel">
   <h2>The proof · procedures traced to root events</h2>
   <div class="sub" style="margin-top:-6px">Each metric = a procedure over your events. Expand a metric → its events; expand an event → the exact diff.</div>
   <div style="margin-top:12px">`;
 for(const d of DATA.trace.derivations){
   body+=`<div class="deriv"><div class="dhead"><span class="nm">${d.signal}</span>
     <span class="pl ${d.pole}">${d.pole}</span>
     <span class="v"><b>${d.numerator}</b> / <b>${d.denominator}</b> = <b>${d.value}</b></span></div>
     <div class="dproc">${esc(d.procedure)}</div>
     ${d.contributors.length?`<details><summary>${d.contributors.length} contributing event${d.contributors.length===1?'':'s'}</summary>
       <div class="evlist">${d.contributors.slice(0,40).map(c=>evRow(d,c)).join("")}${d.contributors.length>40?`<div class="ev">… +${d.contributors.length-40} more</div>`:''}</div></details>`:'<div class="ev">— no events matched —</div>'}
     ${d.lossy?`<div class="dlossy">⚠ ${esc(d.lossy)}</div>`:''}</div>`;
 }
 body+=`</div>
   <div class="foot">value ← procedure ← the tool_call events above ← the raw JSONL transcript. Every field also traces UP: signal → schema → citation → primary source.</div>
 </div>`;
 $("#body").innerHTML=body;
}
document.addEventListener("click",e=>{
 const h=e.target.closest(".sighead");
 if(h){document.getElementById("cit-"+h.dataset.cit).classList.toggle("open");return;}
 const ex=e.target.closest(".exp");
 if(ex){const box=document.getElementById("chg-"+ex.dataset.k);
   if(!box.dataset.l){detail(box,DATA.events[ex.dataset.seq]);box.dataset.l="1";}
   box.classList.toggle("open");ex.textContent=box.classList.contains("open")?"▾ change":"▸ change";}
});
render();
</script></body></html>
"""


def run_tests() -> int:
    # Render a synthetic payload (no network) — assert structure survives.
    synthetic = {
        "session": {"sid": "abc12345", "project": "Demo", "label": "test", "events": 10, "agent": "claude-code"},
        "reading": {"net_lean": -0.3, "deliberate_index": 0.1, "spontaneous_index": 0.4, "n_events": 10,
                    "signals": {k: 0.3 for k in mt.SIGNAL_BACKING}},
        "reading_valid": True, "reading_errs": [],
        "assessment": {"claim": "spontaneous-leaning", "grounds": {"metrics": {}, "n_events": 10, "net_lean": -0.3},
                       "warrant": "w", "backing": [{"theory_id": "dietrich-2004", "warrant_strength": "analogical"}],
                       "qualifier": {"confidence": 0.4, "warrant_strength": "analogical"}, "rebuttal": "r"},
        "assessment_valid": True, "assessment_errs": [],
        "trace": {"derivations": [{"signal": "churn", "pole": "spontaneous", "procedure": "p",
                                   "numerator": 2, "denominator": 5, "value": 0.4, "lossy": "", "contributors": []}]},
        "events": {}, "citations": {k: {"warrant_strength": "analogical", "warrant": "w", "pole": "x",
                                        "primary_source": None, "flaw": ""} for k in mt.SIGNAL_BACKING},
        "theories": {},
    }
    html = render_html(synthetic)
    ok = "<html" in html and "DATA = {" in html and "spontaneous-leaning" in html and "/*__DATA__*/" not in html
    print("ok — session-report render test passed" if ok else "FAIL")
    return 0 if ok else 1


def main() -> int:
    ap = argparse.ArgumentParser(description="Single-session cited HTML report")
    ap.add_argument("session_id", nargs="?")
    ap.add_argument("--self", action="store_true", help="most-recently-active session")
    ap.add_argument("--out", help="output path (default /tmp/session-<id>.html)")
    ap.add_argument("--api", default=pd.DEFAULT_API)
    ap.add_argument("--test", action="store_true")
    args = ap.parse_args()
    if args.test:
        return run_tests()
    sid = args.session_id
    if args.self or not sid:
        sessions = (pd._get(args.api, "/api/sessions") or {}).get("sessions", [])
        if not sessions:
            print("no sessions found"); return 1
        sid = max(sessions, key=lambda s: s.get("last_event", ""))["session_id"]
    data = build_report_data(args.api, sid)
    html = render_html(data)
    out = Path(args.out) if args.out else Path(f"/tmp/session-{sid[:8]}.html")
    out.write_text(html)
    v = "✓" if (data["reading_valid"] and data["assessment_valid"]) else "✗"
    print(f"wrote {out}  ({data['session']['events']} events · "
          f"{data['assessment']['claim']} · schema {v})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
