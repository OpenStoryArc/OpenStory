"""Classify the *shape* of a session's prompt trail by verb mix.

Reads turn.sentence patterns and reduces them to a verb histogram, then
labels the session as one of four archetypes:

- directive: high `wrote`+`committed`, low `explained`. "do X" → ship.
- socratic:  high `explained`, paced `edited`/`wrote`. research → spec → code.
- recovery:  high `checked` and `explained` interleaved without much new write.
- plumbing:  high `wrote`+`edited`, low `explained`. refactor / housekeeping.

The classifier is a small rule set, not ML. It is meant to be readable and
adjustable. See classify().

Usage:
    python3 scripts/prompt_trail.py SESSION_ID
    python3 scripts/prompt_trail.py SESSION_ID --json
    python3 scripts/prompt_trail.py all --csv         # one row per session
    python3 scripts/prompt_trail.py --test
"""

from __future__ import annotations

import argparse
import json
import sys
import urllib.error
import urllib.request
from collections import Counter
from dataclasses import asdict, dataclass


DEFAULT_URL = "http://localhost:3002"

# verb buckets used by the classifier
WRITE_VERBS = {"wrote", "edited"}
SHIP_VERBS = {"ran", "committed"}
EXPLAIN_VERBS = {"explained", "answered", "asked"}
CHECK_VERBS = {"checked", "tested", "investigated", "read"}


def fetch(base_url: str, path: str) -> object:
    try:
        with urllib.request.urlopen(f"{base_url}{path}", timeout=30) as resp:
            return json.loads(resp.read())
    except urllib.error.URLError as e:
        sys.stderr.write(f"error: failed to fetch {path}: {e}\n")
        sys.exit(2)


@dataclass
class TrailShape:
    session_id: str
    sentence_count: int
    verb_mix: dict[str, int]
    write_share: float
    explain_share: float
    check_share: float
    archetype: str
    sample_prompts: list[str]


# -- Classifier ----------------------------------------------------

def classify(verb_mix: Counter) -> tuple[str, float, float, float]:
    """Return (archetype, write_share, explain_share, check_share).

    "ran" + "committed" are the discriminator between directive (ships) and
    plumbing (silent refactor). They are not double-counted in any other
    share.
    """
    total = sum(verb_mix.values()) or 1
    w = sum(verb_mix.get(v, 0) for v in WRITE_VERBS) / total
    e = sum(verb_mix.get(v, 0) for v in EXPLAIN_VERBS) / total
    c = sum(verb_mix.get(v, 0) for v in CHECK_VERBS) / total
    ship = sum(verb_mix.get(v, 0) for v in SHIP_VERBS) / total

    # rules: ordered, first match wins
    if e >= 0.45:
        archetype = "socratic"
    elif (w + ship) >= 0.55 and e < 0.20:
        archetype = "directive" if ship >= 0.10 else "plumbing"
    elif c >= 0.30 and e >= 0.20:
        archetype = "recovery"
    elif w >= 0.40 and e <= 0.30:
        archetype = "directive"
    else:
        archetype = "mixed"
    return archetype, w, e, c


def _human_prompt(meta: dict) -> str:
    """Extract human prompt from sentence metadata. Live shape is metadata.human.content."""
    h = meta.get("human")
    if isinstance(h, dict):
        return h.get("content") or ""
    return meta.get("human_prompt") or ""


def summarize(session_id: str, sentences: list[dict]) -> TrailShape:
    verb_mix: Counter = Counter()
    sample_prompts: list[str] = []
    for s in sentences:
        meta = s.get("metadata") or {}
        verb = (meta.get("verb") or "").lower()
        if verb:
            verb_mix[verb] += 1
        prompt = _human_prompt(meta)
        if prompt and len(sample_prompts) < 5:
            sample_prompts.append(prompt[:160].replace("\n", " / "))
    archetype, w, e, c = classify(verb_mix)
    return TrailShape(
        session_id=session_id,
        sentence_count=len(sentences),
        verb_mix=dict(verb_mix.most_common()),
        write_share=w,
        explain_share=e,
        check_share=c,
        archetype=archetype,
        sample_prompts=sample_prompts,
    )


def for_session(base_url: str, session_id: str) -> TrailShape:
    res = fetch(base_url, f"/api/sessions/{session_id}/patterns?type=turn.sentence")
    sents = res.get("patterns", []) if isinstance(res, dict) else []
    return summarize(session_id, sents)


def all_sessions(base_url: str) -> list[str]:
    res = fetch(base_url, "/api/sessions")
    if isinstance(res, dict):
        return [s["session_id"] for s in res.get("sessions", [])]
    return [s["session_id"] for s in res]


# -- Output --------------------------------------------------------

def fmt_md(t: TrailShape) -> str:
    bars = []
    for verb, n in list(t.verb_mix.items())[:10]:
        bar = "█" * min(n, 30)
        bars.append(f"  {verb:<14} {n:>4}  {bar}")
    sample = "\n".join(f"  - {p}" for p in t.sample_prompts) or "  _(none)_"
    return (
        f"# Trail shape: `{t.session_id}`\n"
        f"**Archetype:** **{t.archetype}**\n"
        f"**Sentences:** {t.sentence_count}  ·  "
        f"write {t.write_share*100:.0f}%  ·  explain {t.explain_share*100:.0f}%  ·  check {t.check_share*100:.0f}%\n\n"
        f"## Verb mix\n" + "\n".join(bars) + "\n\n"
        f"## Sample prompts\n" + sample + "\n"
    )


def fmt_csv(rows: list[TrailShape]) -> str:
    out = ["session_id,sentences,archetype,write_share,explain_share,check_share"]
    for t in rows:
        out.append(",".join([
            t.session_id, str(t.sentence_count), t.archetype,
            f"{t.write_share:.4f}", f"{t.explain_share:.4f}", f"{t.check_share:.4f}",
        ]))
    return "\n".join(out) + "\n"


# -- Self-tests ----------------------------------------------------

def _socratic_sentences() -> list[dict]:
    return [{"metadata": {"verb": v}} for v in
            ["explained","explained","explained","explained","explained",
             "edited","wrote","explained","wrote","checked"]]

def _directive_sentences() -> list[dict]:
    return [{"metadata": {"verb": v}} for v in
            ["wrote","wrote","wrote","wrote","edited","ran","committed","checked"]]

def _recovery_sentences() -> list[dict]:
    return [{"metadata": {"verb": v}} for v in
            ["checked","explained","checked","explained","checked","tested",
             "explained","edited"]]


def selftest() -> int:
    failures = 0
    def check(name, cond, detail=""):
        nonlocal failures
        print(f"  {'ok  ' if cond else 'FAIL'} {name}{('   '+detail) if not cond else ''}")
        if not cond: failures += 1

    print("== classify(socratic) ==")
    s = summarize("s-1", _socratic_sentences())
    check("socratic detected", s.archetype == "socratic", s.archetype)
    check("explain dominant", s.explain_share > s.write_share)

    print("== classify(directive) ==")
    s = summarize("s-2", _directive_sentences())
    check("directive detected", s.archetype == "directive", s.archetype)
    check("write dominant", s.write_share > s.explain_share)

    print("== classify(recovery) ==")
    s = summarize("s-3", _recovery_sentences())
    check("recovery detected", s.archetype == "recovery", s.archetype)
    check("check share high", s.check_share >= 0.30)

    print("== fmt_md ==")
    md = fmt_md(s)
    check("md has archetype", "recovery" in md)
    check("md has bars", "█" in md)

    print("\nFAILED" if failures else "\nall tests passed")
    return 1 if failures else 0


# -- CLI -----------------------------------------------------------

def main() -> None:
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("session_id", nargs="?")
    p.add_argument("--url", default=DEFAULT_URL)
    p.add_argument("--json", action="store_true")
    p.add_argument("--csv", action="store_true")
    p.add_argument("--test", action="store_true")
    args = p.parse_args()

    if args.test:
        sys.exit(selftest())
    if not args.session_id:
        p.error("session_id required (or 'all', --test)")

    if args.session_id == "all":
        sids = all_sessions(args.url)
        rows = [for_session(args.url, sid) for sid in sids]
        rows = [r for r in rows if r.sentence_count > 0]
        if args.json:
            print(json.dumps([asdict(r) for r in rows], indent=2))
        elif args.csv:
            print(fmt_csv(rows))
        else:
            for r in rows:
                print(f"{r.archetype:<10} {r.sentence_count:>4}sent  w={r.write_share*100:>3.0f}% e={r.explain_share*100:>3.0f}% c={r.check_share*100:>3.0f}%  {r.session_id}")
        return

    shape = for_session(args.url, args.session_id)
    if args.json:
        print(json.dumps(asdict(shape), indent=2))
    elif args.csv:
        print(fmt_csv([shape]))
    else:
        print(fmt_md(shape))


if __name__ == "__main__":
    main()
