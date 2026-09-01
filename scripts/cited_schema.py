"""Cited JSON Schemas for the evaluation types — types with roots, exercised by functions.

The idea: every type in the evaluation carries its research citation IN the schema,
so a value's provenance is part of its type, not a comment. The schema is the bridge
between the theory registry (roots) and the procedures (functions).

Convention — any schema or property may carry `x-citation`:
    { "theory_id": <registry id | null>, "warrant_strength": ..., "warrant": ...,
      "pole"?: ..., "flaw"?: ... }
`theory_id` resolves into theory-registry.json (the primary source). `null` means
honestly uncited (metaphorical / heuristic). `warrant_strength` ∈
validated | validated-frame | analogical | metaphorical | none.

The reading schema is GENERATED from `metric_trace.SIGNAL_BACKING` +
`PIPELINE_BACKING` + the registry, so the cited types can never drift from the
procedures that produce them.

    python3 scripts/cited_schema.py --emit             # write schemas/*.json
    python3 scripts/cited_schema.py --audit            # backing ledger, derived from the schema
    python3 scripts/cited_schema.py --explain signals.churn
    python3 scripts/cited_schema.py --validate <SID>   # validate a live session reading
    python3 scripts/cited_schema.py --test
"""

import argparse
import json
import sys
from pathlib import Path

import profile_dimensions as pd
import metric_trace as mt

REGISTRY = Path(__file__).resolve().parent.parent / "docs/research/developer-archetypes/theory-registry.json"
SCHEMA_DIR = Path(__file__).resolve().parent.parent / "docs/research/developer-archetypes/schemas"


def load_registry() -> dict:
    return {r["id"]: r for r in json.loads(REGISTRY.read_text())["records"]}


def cite(reg: dict, theory_id, strength, warrant, **extra) -> dict:
    """Build an x-citation, resolving the primary source from the registry."""
    src = None
    if theory_id and theory_id in reg:
        r = reg[theory_id]
        src = f"{r['scholar']} ({r['year']}). {r['primary_work']}. {r['primary_source']}"
    return {"theory_id": theory_id, "warrant_strength": strength,
            "warrant": warrant, "primary_source": src, **extra}


def num01(citation: dict) -> dict:
    return {"type": "number", "minimum": 0, "maximum": 1, "x-citation": citation}


# ── Generate the cited schema from the code's backing map (single source) ─────
def reading_schema() -> dict:
    reg = load_registry()
    b = mt.PIPELINE_BACKING
    signals = {}
    for name, sb in mt.SIGNAL_BACKING.items():
        signals[name] = num01(cite(reg, sb["theory"], sb["strength"], sb["warrant"],
                                   pole=sb["pole"], flaw=sb["flaw"], scope=sb.get("scope", "session")))
    # A session reading only carries session-scope signals; window-scope signals
    # (e.g. plan_density) are typed + cited but NOT required here. The type makes
    # the scope distinction explicit instead of letting it hide as an always-0 value.
    session_required = [n for n, sb in mt.SIGNAL_BACKING.items() if sb.get("scope", "session") == "session"]
    return {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "openstory://archetype/session-reading",
        "title": "SessionReading",
        "description": "One session's deliberate↔spontaneous reading, fully cited.",
        "type": "object",
        "x-citation": cite(reg, b["process_locus"]["theory"], b["process_locus"]["strength"],
                           b["process_locus"]["note"]),
        "required": ["net_lean", "deliberate_index", "spontaneous_index", "signals"],
        "properties": {
            "net_lean": {"type": "number", "minimum": -1, "maximum": 1,
                "x-citation": cite(reg, b["the_axis"]["theory"], b["the_axis"]["strength"],
                                   "net_lean = deliberate_index − spontaneous_index; "
                                   + b["the_axis"]["note"])},
            "deliberate_index": num01(cite(reg, b["constants"]["theory"], b["constants"]["strength"],
                "mean of 3 deliberate signals — " + b["constants"]["note"])),
            "spontaneous_index": num01(cite(reg, b["constants"]["theory"], b["constants"]["strength"],
                "mean of 4 spontaneous signals — " + b["constants"]["note"])),
            "n_events": {"type": "integer", "minimum": 0,
                "x-citation": cite(reg, "rhodes-1961", "validated",
                                   "count of tool_call CloudEvents (the Process locus)")},
            "signals": {"type": "object", "required": session_required,
                        "properties": signals,
                        "x-citation": cite(reg, "dietrich-2004", "analogical",
                            "each signal is an analogical proxy for one processing-mode pole")},
        },
    }


STRENGTH_ENUM = ["validated", "validated-frame", "analogical", "metaphorical", "none", "refuted"]


def assessment_schema() -> dict:
    """The Toulmin chain as a cited type: claim ← grounds ← warrant ← backing ←
    qualifier ← rebuttal. The STRUCTURE is Toulmin; the discipline is Cronbach-Meehl."""
    reg = load_registry()
    return {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "openstory://archetype/assessment",
        "title": "Assessment",
        "description": "A single traceable verdict — the Toulmin argument, with backing as the citation hook.",
        "type": "object",
        "x-citation": cite(reg, "toulmin-1958", "validated",
            "argument decomposes into claim/grounds/warrant/backing/qualifier/rebuttal"),
        "required": ["claim", "grounds", "warrant", "backing", "qualifier", "rebuttal"],
        "properties": {
            "claim": {"type": "string",
                "x-citation": cite(reg, "dietrich-2004", "analogical", "the verdict label (a processing-mode pole)")},
            "grounds": {"type": "object",
                "x-citation": cite(reg, "rhodes-1961", "validated",
                    "the evidence — process metrics + the events that produced them (downward provenance)"),
                "required": ["metrics"],
                "properties": {
                    "metrics": {"type": "object"},
                    "n_events": {"type": "integer", "minimum": 0},
                    "net_lean": {"type": "number", "minimum": -1, "maximum": 1},
                    "evidence_event_ids": {"type": "array", "items": {"type": "string"}}}},
            "warrant": {"type": "string",
                "x-citation": cite(reg, "dietrich-2004", "analogical",
                    "the bridge: these process signals indicate this processing mode")},
            "backing": {"type": "array",
                "x-citation": cite(reg, "toulmin-1958", "validated",
                    "backing = the evidence beneath the warrant — the citation hook (upward provenance)"),
                "items": {"type": "object", "required": ["theory_id", "warrant_strength"],
                    "properties": {
                        "theory_id": {"type": "string"},
                        "warrant_strength": {"type": "string", "enum": STRENGTH_ENUM},
                        "status": {"type": "string"}}}},
            "qualifier": {"type": "object",
                "x-citation": cite(reg, "cronbach-meehl-1955", "validated",
                    "warrant strength + confidence — 'rationalization is not validation'"),
                "required": ["warrant_strength"],
                "properties": {
                    "confidence": {"type": "number", "minimum": 0, "maximum": 1},
                    "warrant_strength": {"type": "string", "enum": STRENGTH_ENUM}}},
            "rebuttal": {"type": "string",
                "x-citation": cite(reg, "cronbach-meehl-1955", "validated",
                    "the falsifiability hook — what would refute the claim")},
        },
    }


def session_point_schema() -> dict:
    reg = load_registry()
    return {"type": "object", "required": ["sid", "net_lean"],
        "x-citation": cite(reg, "rhodes-1961", "validated", "one session's process reading + metadata"),
        "properties": {
            "sid": {"type": "string"}, "project": {"type": "string"}, "label": {"type": "string"},
            "events": {"type": "integer", "minimum": 0},
            "net_lean": {"type": "number", "minimum": -1, "maximum": 1},
            "signals": {"type": "object"}}}


def distribution_schema() -> dict:
    """The whole report's spread as a cited type — distribution, not a single trait."""
    reg = load_registry()
    return {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "openstory://archetype/session-distribution",
        "title": "SessionDistribution",
        "description": "Per-session readings across a window — the honest unit, not a pooled number.",
        "type": "object",
        "x-citation": cite(reg, "dietrich-2004", "validated-frame",
            "mode is task/episode-dependent (Dietrich); not a single metric (space-2021) → show the spread"),
        "required": ["sessions", "summary"],
        "properties": {
            "sessions": {"type": "array", "items": session_point_schema()},
            "summary": {"type": "object", "required": ["median", "spread"],
                "properties": {
                    "median": {"type": "number", "minimum": -1, "maximum": 1,
                        "x-citation": cite(reg, "space-2021", "validated-frame",
                            "the honest summary — median session, not the volume-weighted pool")},
                    "mean": {"type": "number", "minimum": -1, "maximum": 1},
                    "spread": {"type": "number", "minimum": 0, "maximum": 2},
                    "n": {"type": "integer", "minimum": 0},
                    "event_weighted": {"type": "number", "minimum": -1, "maximum": 1,
                        "x-citation": cite(reg, None, "none",
                            "the MISLEADING pooled number — kept only for contrast; dominated by the biggest sessions")}}},
        },
    }


SCHEMAS = {"session-reading": reading_schema, "assessment": assessment_schema,
           "session-distribution": distribution_schema}


# ── Functions that EXERCISE the cited types ───────────────────────────────────
def _walk(schema: dict, path: str):
    """Resolve a dotted path (e.g. 'signals.churn') to its subschema."""
    node = schema
    for part in [p for p in path.split(".") if p]:
        props = node.get("properties", {})
        if part not in props:
            return None
        node = props[part]
    return node


def explain(path: str) -> str:
    """The citation chain for any field across ANY schema — type with its clear root."""
    node = None; schema_name = "session-reading"
    for name, builder in SCHEMAS.items():
        cand = _walk(builder(), path) if path else builder()
        if cand is not None and cand.get("x-citation"):
            node, schema_name = cand, name
            break
    if node is None:
        return f"no cited field matches: {path}"
    c = node.get("x-citation")
    L = [f"FIELD  {schema_name} :: {path or '(root)'}",
         f"  type            {node.get('type')}" + (f" [{node.get('minimum')}..{node.get('maximum')}]"
              if 'minimum' in node else ""),
         f"  pole            {c.get('pole', '—')}",
         f"  warrant         {c['warrant']}",
         f"  warrant_strength {c['warrant_strength']}",
         f"  root            {c['primary_source'] or '⚠ UNCITED (heuristic / metaphorical)'}"]
    if c.get("flaw"):
        L.append(f"  known flaw      {c['flaw']}")
    return "\n".join(L)


def audit() -> str:
    """The WHOLE-REPORT backing ledger, DERIVED from every cited schema."""
    rows = []
    def visit(node, path):
        c = node.get("x-citation")
        if c:
            rows.append((path or "(root)", c["warrant_strength"], c.get("theory_id") or "—"))
        for k, v in node.get("properties", {}).items():
            visit(v, f"{path}.{k}" if path else k)
        if isinstance(node.get("items"), dict):
            visit(node["items"], f"{path}[]")
    for name, builder in SCHEMAS.items():
        visit(builder(), name)
    order = {"validated": 0, "validated-frame": 1, "analogical": 2, "metaphorical": 3, "none": 4}
    rows.sort(key=lambda r: order.get(r[1], 9))
    L = ["BACKING LEDGER (generated from the cited schema)", "=" * 60,
         f"{'field':<26}{'strength':<18}root"]
    for path, strength, tid in rows:
        L.append(f"{path:<26}{strength:<18}{tid}")
    counts = {}
    for _, s, _ in rows:
        counts[s] = counts.get(s, 0) + 1
    L.append("-" * 60)
    L.append("  ·  ".join(f"{s}: {n}" for s, n in sorted(counts.items(), key=lambda kv: order.get(kv[0], 9))))
    return "\n".join(L)


def validate(data: dict, schema: dict = None, path: str = "") -> list[str]:
    """Minimal stdlib validator — exercises the type contract (incl. ranges)."""
    schema = schema or reading_schema()
    errs = []
    t = schema.get("type")
    if t == "object":
        if not isinstance(data, dict):
            return [f"{path or '(root)'}: expected object"]
        for req in schema.get("required", []):
            if req not in data:
                errs.append(f"{path}.{req}".lstrip(".") + ": required, missing")
        for k, sub in schema.get("properties", {}).items():
            if k in data:
                errs += validate(data[k], sub, f"{path}.{k}".lstrip("."))
    elif t in ("number", "integer"):
        if not isinstance(data, (int, float)) or isinstance(data, bool):
            errs.append(f"{path}: expected {t}")
        else:
            if "minimum" in schema and data < schema["minimum"]:
                errs.append(f"{path}: {data} < minimum {schema['minimum']}")
            if "maximum" in schema and data > schema["maximum"]:
                errs.append(f"{path}: {data} > maximum {schema['maximum']}")
    elif t == "string":
        if not isinstance(data, str):
            errs.append(f"{path}: expected string")
        elif "enum" in schema and data not in schema["enum"]:
            errs.append(f"{path}: '{data}' not in enum {schema['enum']}")
    elif t == "array":
        if not isinstance(data, list):
            errs.append(f"{path}: expected array")
        elif isinstance(schema.get("items"), dict):
            for i, el in enumerate(data):
                errs += validate(el, schema["items"], f"{path}[{i}]")
    return errs


# ── Compose real cited structures from live data (the report, as types) ───────
def assessment_for(api: str, sid: str) -> dict:
    """Build a cited Assessment (Toulmin chain) for one session, from live events."""
    import archetype_report as ar
    journey = pd._get(api, f"/api/sessions/{sid}/tool-journey") or []
    reading = mt.session_reading(mt.steps_from_journey(journey if isinstance(journey, list) else []))
    return {
        "claim": ar.lean_label(reading["net_lean"]),
        "grounds": {"metrics": reading["signals"], "n_events": reading["n_events"],
                    "net_lean": reading["net_lean"], "evidence_event_ids": []},
        "warrant": "deliberate↔spontaneous process signals place this session on a processing mode",
        "backing": [{"theory_id": "dietrich-2004", "warrant_strength": "analogical"}],
        "qualifier": {"confidence": round(min(0.6, abs(reading["net_lean"]) + 0.1), 2),
                      "warrant_strength": "analogical"},
        "rebuttal": "Refuted if these signals don't track an independent measure of "
                    "controlled-vs-associative work (Cronbach & Meehl's nomological-net test).",
    }


def distribution_for(api: str, days: int, sample: int) -> dict:
    """Build a cited SessionDistribution from the window (reuses session_distribution)."""
    import session_distribution as sd
    import statistics
    readings = sd.distribution(api, days, sample)
    leans = [r["net_lean"] for r in readings]
    ev = sum(r["net_lean"] * r["events"] for r in readings) / max(1, sum(r["events"] for r in readings))
    return {
        "sessions": readings,
        "summary": {"median": round(statistics.median(leans), 3), "mean": round(statistics.mean(leans), 3),
                    "spread": round(max(leans) - min(leans), 3), "n": len(readings),
                    "event_weighted": round(ev, 3)},
    }


def emit() -> str:
    SCHEMA_DIR.mkdir(parents=True, exist_ok=True)
    written = []
    for name, builder in SCHEMAS.items():
        p = SCHEMA_DIR / f"{name}.schema.json"
        p.write_text(json.dumps(builder(), indent=2, ensure_ascii=False) + "\n")
        written.append(str(p.relative_to(Path(__file__).resolve().parent.parent)))
    return "wrote:\n  " + "\n  ".join(written)


def run_tests() -> int:
    fails = []
    def chk(n, c): (None if c else fails.append(n))
    schema = reading_schema()
    # every signal in the procedures has a cited property in the schema
    sigprops = schema["properties"]["signals"]["properties"]
    chk("all-signals-typed", set(sigprops) == set(mt.SIGNAL_BACKING))
    chk("every-signal-cited", all("x-citation" in v for v in sigprops.values()))
    # a valid reading passes; an out-of-range one fails
    good = {"net_lean": -0.45, "deliberate_index": 0.08, "spontaneous_index": 0.53,
            "n_events": 90, "signals": {k: 0.5 for k in mt.SIGNAL_BACKING}}
    chk("valid-passes", validate(good) == [])
    bad = {**good, "net_lean": 2.0, "signals": {**good["signals"], "churn": 1.5}}
    chk("range-caught", any("net_lean" in e for e in validate(bad)) and any("churn" in e for e in validate(bad)))
    miss = {"deliberate_index": 0.1}
    chk("required-caught", any("net_lean" in e for e in validate(miss)))
    # explain resolves a primary source for a cited signal, flags an uncited one
    chk("explain-cited", "Dietrich" in explain("signals.context_switch"))
    chk("explain-uncited", "UNCITED" in explain("signals.churn"))   # churn has theory_id null
    # assessment schema: Toulmin chain validates, enum + required enforced
    asch = assessment_schema()
    good_a = {"claim": "spontaneous-leaning", "grounds": {"metrics": {}}, "warrant": "w",
              "backing": [{"theory_id": "dietrich-2004", "warrant_strength": "analogical"}],
              "qualifier": {"warrant_strength": "analogical"}, "rebuttal": "r"}
    chk("assess-valid", validate(good_a, asch) == [])
    bad_a = {**good_a, "backing": [{"theory_id": "x", "warrant_strength": "bogus"}]}
    chk("assess-enum", any("enum" in e for e in validate(bad_a, asch)))
    chk("assess-required", any("rebuttal" in e for e in validate({k: v for k, v in good_a.items() if k != "rebuttal"}, asch)))
    chk("explain-toulmin", "Toulmin" in explain("backing") or "toulmin" in explain("backing").lower())
    # distribution schema: array of session points validates
    dsch = distribution_schema()
    good_d = {"sessions": [{"sid": "a", "net_lean": -0.3}], "summary": {"median": -0.3, "spread": 0.7}}
    chk("dist-valid", validate(good_d, dsch) == [])
    bad_d = {"sessions": [{"sid": "a", "net_lean": 5}], "summary": {"median": -0.3, "spread": 0.7}}
    chk("dist-range", any("net_lean" in e for e in validate(bad_d, dsch)))
    if fails:
        print("FAIL:", ", ".join(fails)); return 1
    print("ok — cited-schema tests passed"); return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Cited JSON Schemas for the evaluation types")
    ap.add_argument("--emit", action="store_true", help="write schemas/*.json")
    ap.add_argument("--audit", action="store_true", help="backing ledger from the schema")
    ap.add_argument("--explain", metavar="PATH", help="citation chain for a field, e.g. signals.churn")
    ap.add_argument("--validate", metavar="SID", help="validate a live session reading")
    ap.add_argument("--validate-assessment", metavar="SID", help="build + validate a Toulmin assessment")
    ap.add_argument("--validate-distribution", action="store_true", help="build + validate the window distribution")
    ap.add_argument("--days", type=int, default=30)
    ap.add_argument("--sample", type=int, default=60)
    ap.add_argument("--api", default=pd.DEFAULT_API)
    ap.add_argument("--test", action="store_true")
    args = ap.parse_args()

    if args.test:
        return run_tests()
    if args.emit:
        print(emit()); return 0
    if args.audit:
        print(audit()); return 0
    if args.explain is not None:
        print(explain(args.explain)); return 0

    def report(obj, schema, name):
        errs = validate(obj, schema)
        print(json.dumps(obj, indent=2, ensure_ascii=False)[:1400] + ("\n…" if len(json.dumps(obj)) > 1400 else ""))
        print(f"\nvalidation: " + (f"✓ conforms to {name}.schema.json" if not errs else "✗ " + "; ".join(errs)))
        return 0 if not errs else 1

    if args.validate:
        journey = pd._get(args.api, f"/api/sessions/{args.validate}/tool-journey") or []
        reading = mt.session_reading(mt.steps_from_journey(journey if isinstance(journey, list) else []))
        return report(reading, reading_schema(), "session-reading")
    if args.validate_assessment:
        return report(assessment_for(args.api, args.validate_assessment), assessment_schema(), "assessment")
    if args.validate_distribution:
        return report(distribution_for(args.api, args.days, args.sample), distribution_schema(), "session-distribution")
    ap.print_help(); return 0


if __name__ == "__main__":
    sys.exit(main())
