#!/usr/bin/env bash
set -euo pipefail

# daystory — tell the story of a day from OpenStory session data
# Usage: daystory.sh [YYYY-MM-DD]  (defaults to today)

API="${OPEN_STORY_API_URL:-http://open-story:3002}"

DATE="${1:-$(date -u +%Y-%m-%d)}"

fetch() {
  local url="$1"
  local token="${OPEN_STORY_API_TOKEN:-}"
  if [ -n "$token" ]; then
    curl -sf -H "Authorization: Bearer $token" "$url"
  else
    curl -sf "$url"
  fi
}

fetch "$API/api/sessions" | python3 -c "
import json, sys, signal, urllib.request
from datetime import datetime, timedelta
signal.signal(signal.SIGPIPE, signal.SIG_DFL)

date_str = '$DATE'
sessions = json.load(sys.stdin)

day_start = datetime.fromisoformat(date_str + 'T00:00:00')
day_end = day_start + timedelta(days=1)
weekday = day_start.strftime('%A')

# Find sessions that overlap with the target date
day_sessions = []
for s in sessions:
    start = datetime.fromisoformat(s['start_time'].replace('Z',''))
    if start < day_end and start >= day_start - timedelta(days=3):
        day_sessions.append(s)

if not day_sessions:
    print(f'Nothing happened on {weekday}, {date_str}.')
    sys.exit(0)

# Gather all records for the day across sessions
all_user = []
all_asst = []
all_tools = {}
all_tokens = 0
all_records = 0

for s in day_sessions:
    sid = s['session_id']
    url = '$API/api/sessions/' + sid + '/records'
    try:
        resp = urllib.request.urlopen(url)
        records = json.loads(resp.read())
    except:
        continue

    for r in records:
        ts = r.get('timestamp', '')
        if not ts.startswith(date_str):
            continue
        all_records += 1
        rt = r.get('record_type', '')
        p = r.get('payload', {})

        if rt == 'user_message':
            content = p.get('content','')
            if content.startswith('System:') or content.startswith('Read HEARTBEAT'):
                continue
            all_user.append(r)
        elif rt == 'assistant_message':
            all_asst.append(r)
        elif rt == 'tool_call':
            name = p.get('name', '') or 'unknown'
            all_tools[name] = all_tools.get(name, 0) + 1
        elif rt == 'token_usage':
            all_tokens += p.get('total_tokens', 0)

if not all_user:
    print(f'Nothing happened on {weekday}, {date_str}.')
    sys.exit(0)

# === Find the story ===

# Opening: first human message
opening = all_user[0].get('payload',{}).get('content','')[:200]
open_time = all_user[0].get('timestamp','')[11:16]

# Closing: last human message  
closing = all_user[-1].get('payload',{}).get('content','')[:200]
close_time = all_user[-1].get('timestamp','')[11:16]

# Find the turn: the moment the conversation shifted
# Look for the longest message, the most surprising topic shift,
# or a reframe — questions, philosophical shifts, corrections
turns = []
for i, r in enumerate(all_user):
    content = r.get('payload',{}).get('content','')
    ts = r.get('timestamp','')[11:16]
    
    # Detect reframes and pivots
    lower = content.lower()
    is_question = '?' in content
    is_reframe = any(w in lower for w in ['i prefer', 'actually', 'what about', 'what if', 'i think', 'i wonder', 'how about', 'what is the definition'])
    is_pivot = any(w in lower for w in ['now,', 'ok so', 'let\\'s', 'shifting', 'different', 'curious about'])
    is_deep = any(w in lower for w in ['personhood', 'sovereignty', 'embodied', 'spirit', 'emerge', 'mystery', 'god', 'theology', 'humility'])
    
    score = 0
    if is_question: score += 1
    if is_reframe: score += 2
    if is_pivot: score += 1
    if is_deep: score += 3
    if len(content) > 100: score += 1
    
    if score >= 2:
        turns.append((ts, content[:200], score))

# Sort by score, take top moments
turns.sort(key=lambda x: -x[2])
key_moments = turns[:5]
# Re-sort by time
key_moments.sort(key=lambda x: x[0])

# Find surprises in agent responses
surprises = []
for r in all_asst:
    content = r.get('payload',{}).get('content','')
    if isinstance(content, list):
        for c in content:
            if isinstance(c, dict):
                content = c.get('text', '')
                break
        else:
            content = str(content)
    ts = r.get('timestamp','')[11:16]
    lower = str(content).lower()
    
    if any(w in lower for w in ['oh wow', 'that stops me', 'honestly?', 'genuinely', 'i notice', 'i don\\'t know']):
        # Clean: take first sentence only, strip markdown
        clean = str(content).replace('**','').replace('*','').replace('#','').strip()
        first_sentence = clean.split('.')[0].split('\\n')[0].strip()
        if len(first_sentence) > 15:
            surprises.append((ts, first_sentence))

# Energy and resistance markers
energy = []  # consolation — breakthroughs, excitement, rapid progress
resistance = []  # desolation — errors, limits, long gaps, frustration

prev_ts = None
for r in sorted(all_user + all_asst, key=lambda x: x.get('timestamp','')):
    content = r.get('payload',{}).get('content','')
    if isinstance(content, list):
        for c in content:
            if isinstance(c, dict):
                content = c.get('text','')
                break
        else:
            content = str(content)
    ts_str = r.get('timestamp','')
    ts_short = ts_str[11:16]
    lower = str(content).lower()
    rt = r.get('record_type','')
    
    # Energy: excitement, breakthroughs, creation
    if any(w in lower for w in ['we did it', 'hell yeah', 'love that', '🎉', '🤩', '🥰', 'amazing', 'fantastic', 'very happy', 'stoked', 'sweet', 'fun']):
        snip = str(content).strip()[:80]
        if snip:
            energy.append((ts_short, snip))
    
    # Resistance: errors, limits, frustration
    if any(w in lower for w in ['rate limit', 'error', 'failed', 'crimping', 'still limited', 'stuck', 'can\\'t', 'lost']):
        snip = str(content).strip()[:80]
        if snip:
            resistance.append((ts_short, snip))

    # Long gaps as resistance (> 2 hours)
    if prev_ts and ts_str:
        try:
            from datetime import datetime
            t1 = datetime.fromisoformat(prev_ts.replace('Z',''))
            t2 = datetime.fromisoformat(ts_str.replace('Z',''))
            gap = (t2 - t1).total_seconds()
            if gap > 7200:
                resistance.append((ts_short, f'{int(gap/3600)} hour gap'))
        except:
            pass
    prev_ts = ts_str

# === Tell the story ===

print(f'{weekday}, {date_str}')
print()

# Stats as texture, not the point
tool_str = ', '.join(f'{n} {c}x' for n, c in sorted(all_tools.items(), key=lambda x: -x[1])[:3])
print(f'{len(all_user)} turns. {all_records} records. {tool_str}.')
print()

# The opening
print(f'It started at {open_time}:')
print(f'  \"{opening}\"')
print()

# The key moments — skip the opener, it's already shown
if key_moments:
    for ts, content, score in key_moments:
        if content[:50] == opening[:50]:
            continue
        print(f'{ts}:')
        print(f'  \"{content}\"')
        print()

# Agent surprise moments
if surprises:
    for ts, content in surprises[:2]:
        # Clean up content
        content = content.strip()
        if content and len(content) > 10:
            print(f'{ts} (agent):')
            print(f'  \"{content}\"')
            print()

# The closing
if closing != opening:
    print(f'By {close_time}:')
    print(f'  \"{closing}\"')
    print()

# Energy and resistance
if energy:
    print('Energy:')
    for ts, snip in energy[:4]:
        print(f'  {ts} — {snip}')
    print()

if resistance:
    print('Resistance:')
    for ts, snip in resistance[:4]:
        print(f'  {ts} — {snip}')
    print()

# Closing question — generated from the day's arc
if all_user and len(all_user) > 2:
    first_topic = opening[:40].lower().strip('?.,! ')
    last_topic = closing[:40].lower().strip('?.,! ')
    if first_topic != last_topic:
        print(f'You started with \"{opening[:50].strip()}\" and ended with \"{closing[:50].strip()}\" — what happened?')
    else:
        print(f'What mattered today?')
    print()

print('—')
print()
"
