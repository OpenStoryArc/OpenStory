---
marp: true
theme: default
paginate: true
size: 16:9
---
<style>
@import url('https://fonts.googleapis.com/css2?family=Public+Sans:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500;600&family=Castoro:ital@0;1&display=swap');
:root{--bg:#ffffff;--bg2:#f2f5f8;--panel:#ffffff;--panel2:#f2f5f8;--line:#d8e0ea;--ink:#13233d;--ink2:#3c4a5e;--dim:#79869a;--blue:#0074cc;--cyan:#1f7a8c;--green:#266150;--purple:#6b3fa0;--orange:#e08a00;--red:#de354c;--grad1:#144175;--grad2:#f9a21c;--glow:#14417510;--shadow:0 1px 2px rgba(16,32,60,.05), 0 10px 30px rgba(16,32,60,.06);--r:10px;--fsans:"Public Sans",Inter,system-ui,sans-serif;--fdisplay:"Castoro",Georgia,serif;}
/* per-section accent — every chart derives its colour from this */
  .blue{--ac:var(--blue)} .cyan{--ac:var(--cyan)} .purple{--ac:var(--purple)}
  .green{--ac:var(--green)} .orange{--ac:var(--orange)} .red{--ac:var(--red)}
/* report canvas */
  .viz { margin-top:14px; padding:20px; background:var(--bg2); border:1px solid var(--line); border-radius:var(--r); display:flex; flex-direction:column; gap:20px; }
  .viz .src { color:var(--dim); font:500 11px/1.4 "JetBrains Mono",monospace; border-top:1px solid var(--line); padding-top:11px; }
  .vtitle { font:600 10.5px/1 "JetBrains Mono",monospace; letter-spacing:.14em; text-transform:uppercase; color:var(--dim); margin-bottom:12px; }
  /* stats */
  .vstats { display:flex; flex-wrap:wrap; gap:10px; }
  .vstats .st { flex:1; min-width:96px; background:var(--panel); border:1px solid var(--line); border-radius:10px; padding:13px 15px; box-shadow:var(--shadow); }
  .vstats .st .n { font:700 25px/1 Inter,sans-serif; letter-spacing:-.02em; font-variant-numeric:tabular-nums; }
  .vstats .st.ac .n { color:var(--ac); }
  .vstats .st .l { margin-top:6px; color:var(--dim); font:500 11px/1.3 "JetBrains Mono",monospace; }
  /* bars */
  .vbars { display:flex; flex-direction:column; gap:10px; }
  .vbar { display:grid; grid-template-columns:128px 1fr 46px; gap:14px; align-items:center; }
  .vbar .bl { font-size:12.5px; color:var(--ink2); }
  .vbar .bt { display:block; height:22px; background:color-mix(in srgb, var(--ink) 6%, transparent); border-radius:6px; overflow:hidden; }
  .vbar .bf { display:block; height:100%; background:var(--ac); border-radius:6px; min-width:4px; }
  .vbar .bv { font:600 12.5px/1 "JetBrains Mono",monospace; color:var(--ink); font-variant-numeric:tabular-nums; text-align:right; }
  /* table */
  .vtable { width:100%; border-collapse:collapse; font-size:12.5px; }
  .vtable th { text-align:left; font:600 9.5px/1 "JetBrains Mono",monospace; letter-spacing:.1em; text-transform:uppercase; color:var(--dim); padding:0 0 10px 0; border-bottom:1px solid var(--line); }
  .vtable td { padding:10px 0; border-bottom:1px solid var(--line); color:var(--ink2); vertical-align:middle; }
  .vtable th + th, .vtable td + td { padding-left:20px; }   /* guarantees columns never collide */
  .vtable tr:last-child td { border-bottom:none; }
  .vtable td.r, .vtable th.r { text-align:right; font-variant-numeric:tabular-nums; white-space:nowrap; }
  .chip-sev { font:600 10px/1 "JetBrains Mono",monospace; padding:3px 8px; border-radius:5px; border:1px solid; }
  .chip-sev.hi { color:var(--red); border-color:color-mix(in srgb, var(--red) 45%, var(--line)); background:color-mix(in srgb, var(--red) 12%, transparent); }
  .chip-sev.med { color:var(--orange); border-color:color-mix(in srgb, var(--orange) 45%, var(--line)); background:color-mix(in srgb, var(--orange) 10%, transparent); }
  .chip-sev.lo { color:var(--dim); border-color:var(--line); }
  .dot { margin-right:8px; font-size:11px; } .dot.on { color:var(--green); } .dot.off { color:var(--dim); }
  .cellbar { display:inline-block; width:64px; height:8px; background:color-mix(in srgb, var(--ink) 9%, transparent); border-radius:3px; overflow:hidden; vertical-align:middle; margin-right:9px; }
  .cellbar > span { display:block; height:100%; background:var(--ac); }
  /* timeline */
  .vtl { position:relative; padding-left:24px; }
  .vtl::before { content:""; position:absolute; left:4px; top:6px; bottom:6px; width:2px; background:var(--line); }
  .vtl .ev { position:relative; padding:0 0 16px; }
  .vtl .ev:last-child { padding-bottom:0; }
  .vtl .ev::before { content:""; position:absolute; left:-24px; top:3px; width:9px; height:9px; border-radius:50%; background:var(--bg2); border:2px solid var(--ac); }
  .vtl .d { font:600 11px/1 "JetBrains Mono",monospace; color:var(--ac); }
  .vtl .t { font-size:13.5px; color:var(--ink); margin-top:4px; font-weight:600; }
  .vtl .nt { font-size:12.5px; color:var(--dim); margin-top:2px; line-height:1.45; }
  .tlarc { margin-top:6px; padding-left:13px; border-left:2px solid var(--ac); color:var(--ink2); font-size:13px; font-style:italic; line-height:1.5; }
  /* cards */
  .vcards { display:flex; flex-direction:column; gap:10px; }
  .vcard { background:var(--panel); border:1px solid var(--line); border-left:2px solid var(--ac); border-radius:0 10px 10px 0; padding:13px 16px; box-shadow:var(--shadow); }
  .vcard .ch { font-weight:600; font-size:13.5px; color:var(--ink); }
  .vcard .cs { font:500 11px/1 "JetBrains Mono",monospace; color:var(--dim); margin-left:8px; }
  .vcard .ct { margin-top:6px; color:var(--dim); font-size:12.5px; line-height:1.5; }
  /* groups */
  .vgroups { display:grid; grid-template-columns:repeat(3,1fr); gap:16px; }
  .vgroup .gh { font:600 10px/1 "JetBrains Mono",monospace; text-transform:uppercase; letter-spacing:.1em; color:var(--dim); margin-bottom:9px; padding-bottom:7px; border-bottom:1px solid var(--line); }
  .vgroup.warn .gh { color:var(--orange); }
  .vgroup ul { margin:0; padding-left:16px; color:var(--ink2); font-size:12.5px; line-height:1.5; }
  .vgroup li { margin-bottom:5px; }
  /* meta */
  .vmeta { display:grid; grid-template-columns:auto 1fr; gap:9px 18px; font-size:13px; margin:0; }
  .vmeta dt { color:var(--dim); font:600 10px/1.6 "JetBrains Mono",monospace; text-transform:uppercase; letter-spacing:.06em; }
  .vmeta dd { margin:0; color:var(--ink2); line-height:1.45; }
  /* steps + code */
  .vsteps { counter-reset:s; display:flex; flex-direction:column; gap:9px; }
  .vstep { display:grid; grid-template-columns:23px 1fr; gap:12px; align-items:center; }
  .vstep::before { counter-increment:s; content:counter(s); width:23px; height:23px; border-radius:50%; background:var(--panel); border:1px solid var(--ac); color:var(--ac); font:600 11px/23px "JetBrains Mono",monospace; text-align:center; }
  .vstep code { font-family:"JetBrains Mono",monospace; font-size:12px; color:var(--ink2); }
  .vcode { background:var(--panel); border:1px solid var(--line); border-left:2px solid var(--ac); border-radius:0 8px 8px 0; padding:13px 16px; white-space:pre; overflow-x:auto; font-family:"JetBrains Mono",monospace; font-size:12px; line-height:1.7; color:var(--ink2); }
  /* list */
  .vlist { display:flex; flex-direction:column; }
  .vlist .li { display:grid; grid-template-columns:60px 1fr auto; gap:14px; padding:8px 0; border-bottom:1px solid var(--line); font-size:12.5px; align-items:center; color:var(--ink2); }
  .vlist .li:last-child { border-bottom:none; }
  .vlist .li .ld { font:600 11px/1 "JetBrains Mono",monospace; color:var(--ac); }
  .vlist .li .lm { font:600 11px/1 "JetBrains Mono",monospace; color:var(--dim); }
  /* feed */
  .vfeed { display:flex; flex-direction:column; }
  .vfeed .fr { display:grid; grid-template-columns:66px 56px 1fr; gap:12px; padding:7px 0; border-bottom:1px solid var(--line); font-size:12px; align-items:baseline; }
  .vfeed .fr:last-child { border-bottom:none; }
  .vfeed .ft { font:600 11px/1.4 "JetBrains Mono",monospace; color:var(--dim); }
  .vfeed .fk { font:600 11px/1.4 "JetBrains Mono",monospace; color:var(--ac); }
  .vfeed .fx { color:var(--ink2); }
  /* chips + note */
  .vchips { display:flex; flex-wrap:wrap; gap:7px; align-items:center; }
  .vchips .cl { font:600 10px/1 "JetBrains Mono",monospace; text-transform:uppercase; letter-spacing:.1em; color:var(--dim); margin-right:4px; }
  .vchip { background:var(--panel); border:1px solid var(--line); border-radius:6px; padding:5px 10px; font:500 11.5px/1 "JetBrains Mono",monospace; color:var(--ink2); }
  .vnote { border-left:2px solid var(--ac); padding:3px 0 3px 14px; color:var(--ink2); font-size:13px; line-height:1.55; }
  .vnote.warn { border-color:var(--orange); }
  .vnote b { color:var(--ink); font-weight:600; }


  section { background:radial-gradient(1200px 640px at 84% -14%, var(--glow), transparent), var(--bg);
    color:var(--ink); font-family:var(--fsans); font-size:21px; padding:40px 60px; --ac:var(--blue);
    justify-content:flex-start !important; }   /* top-align (beat theme's centering) */
  section .viz { padding:16px 20px; }            /* slightly tighter canvas on slides */
  section .vtl .ev { padding-bottom:10px; }      /* fit the densest 7-event timeline */
  section h1, section h2, section h3 { font-family:var(--fdisplay); color:var(--ink); letter-spacing:-.02em; font-weight:700; }
  section code { font-family:"JetBrains Mono",monospace; background:var(--bg2); border:1px solid var(--line); border-radius:6px; padding:1px 6px; font-size:.8em; color:var(--ink2); }
  section::after { color:var(--dim); font-family:"JetBrains Mono",monospace; font-size:13px; }
  /* breadcrumb */
  .crumb { display:flex; align-items:center; gap:10px; font:600 12.5px/1 "JetBrains Mono",monospace; letter-spacing:.12em; text-transform:uppercase; color:var(--dim); margin-bottom:14px; }
  .crumb .ix { color:var(--ac); }
  .crumb .tag { margin-left:auto; font-size:10px; padding:4px 10px; border:1px solid var(--line); border-radius:20px; color:var(--dim); letter-spacing:.1em; }
  .crumb .tag.real { color:var(--green); border-color:color-mix(in srgb,var(--green) 45%,var(--line)); }
  .crumb .tag.illus { color:var(--orange); border-color:color-mix(in srgb,var(--orange) 45%,var(--line)); }
  /* prompt headline */
  .q { font-family:var(--fdisplay); font-size:29px; line-height:1.16; color:var(--ink); margin:0 0 18px; font-weight:700; }
  .q .qn { color:var(--ac); margin-right:12px; font-variant-numeric:tabular-nums; }
  /* block flow (not flex) so a <table> child resolves width:100% to full width */
  section .viz { margin-top:0; display:block; }
  section .viz > * + * { margin-top:18px; }
  section .viz .vstats .st .n { font-size:28px; }
  /* Marp's default theme styles raw section table/ul/code at ID specificity;
     !important reclaims our clean chart styling regardless of that. */
  section .vtable { display:table !important; width:100% !important; overflow:visible !important; table-layout:fixed; border-collapse:collapse; }
  section .vtable tr { background:transparent !important; border:0 !important; }
  section .vtable th, section .vtable td { border:0 !important; border-bottom:1px solid var(--line) !important; padding:9px 0 !important; }
  section .vtable th { padding-bottom:11px !important; }
  section .vtable th + th, section .vtable td + td { padding-left:18px !important; }
  section .vtable td.r, section .vtable th.r { text-align:right !important; white-space:nowrap; }
  section .vtable tr:last-child td { border-bottom:0 !important; }
  section .vgroup ul, section .vgroup li { margin:0 !important; }
  section .vgroup ul { padding-left:16px !important; }
  section .vstep code { background:none !important; border:0 !important; padding:0 !important; }
  section h2 { border:0 !important; }
  /* lead / title */
  section.lead { background:linear-gradient(150deg, var(--grad1), var(--grad2)); color:#fff; justify-content:center; }
  section.lead .eyebrow { font:600 14px/1 "JetBrains Mono",monospace; letter-spacing:.24em; text-transform:uppercase; color:rgba(255,255,255,.82); margin-bottom:22px; }
  section.lead .hero { font-size:64px; line-height:1.04; color:#fff; margin:0 0 24px; max-width:18ch; }
  section.lead .lead-sub { font-size:22px; line-height:1.5; color:rgba(255,255,255,.94); max-width:58ch; margin:0; }
  section.lead .lead-foot { margin-top:32px; font:600 13px/1 "JetBrains Mono",monospace; letter-spacing:.12em; color:rgba(255,255,255,.78); }
  section.lead::after { color:rgba(255,255,255,.6); }
  /* contents — all 20 on one slide */
  section.contents { padding:32px 56px; }
  section.contents h2 { font-size:27px; margin:0 0 3px; }
  section.contents .lede { color:var(--dim); font-size:13px; margin:0 0 14px; }
  .agenda { columns:2; column-gap:44px; }
  .ag-sec { break-inside:avoid; margin:0 0 12px; }
  .ag-h { display:flex; align-items:baseline; gap:8px; margin:0 0 5px; padding-bottom:4px; border-bottom:1px solid var(--line); }
  .ag-h .ix { font:700 11.5px/1 "JetBrains Mono",monospace; color:var(--ac); }
  .ag-h .t { font:700 12px/1 var(--fsans); text-transform:uppercase; letter-spacing:.05em; color:var(--ink2); }
  .ag-item { display:flex; gap:10px; align-items:baseline; padding:2.5px 0; font-size:12.5px; color:var(--ink2); line-height:1.32; text-decoration:none; break-inside:avoid; }
  .ag-item:hover { color:var(--ac); }
  .ag-item .n { flex:none; width:20px; font:700 11.5px/1.32 "JetBrains Mono",monospace; color:var(--ac); font-variant-numeric:tabular-nums; }
  .lede strong { color:var(--ink); font-weight:700; }
  /* skills grid (closing slide) */
  .skills { display:grid; grid-template-columns:repeat(3,1fr); gap:14px; margin-top:8px; }
  .skill { background:var(--panel); border:1px solid var(--line); border-left:3px solid var(--ac); border-radius:var(--r); padding:14px 16px; box-shadow:var(--shadow); }
  .skill .cmd { display:block; font:700 18px/1 "JetBrains Mono",monospace; color:var(--ac); }
  .skill .sd { display:block; margin-top:8px; font-size:13px; line-height:1.4; color:var(--ink2); }
</style>

<!-- _class: lead -->
<!-- _paginate: false -->

<div class="eyebrow">OpenStory · draft</div>
<div class="hero">Empower yourself with your own history</div>
<div class="lead-sub">Twenty ways to point OpenStory at your own work — to know yourself, sense your team, and never lose what you&#x27;ve already figured out. Every prompt is distilled from how OpenStory actually gets used, and each opens a real report, charted from a live store.</div>
<div class="lead-foot">20 prompts · 6 ways to point it at your own work</div>

---

<!-- _class: contents -->

## Contents — 20 prompts

<p class="lede">Paste any of these to your coding agent — each runs against your own OpenStory and opens a real report.</p>
<div class="agenda"><div class="ag-sec blue"><div class="ag-h"><span class="ix">01</span><span class="t">Know your own work</span></div><a class="ag-item" href="#3"><span class="n">01</span><span>Summarize everything I&#x27;ve worked on this week across all my projects — group it by project and tell me what actually shipped.</span></a><a class="ag-item" href="#4"><span class="n">02</span><span>What have my agent sessions cost me? Give me the total spend, and a tokens-per-day timeline.</span></a><a class="ag-item" href="#5"><span class="n">03</span><span>Before I share my session history with anyone, scan it for anything sensitive — secrets, API keys, private details, or anything I&#x27;d be embarrassed to expose.</span></a><a class="ag-item" href="#6"><span class="n">04</span><span>Which tools and commands do I rely on most, and where does my time actually go?</span></a></div><div class="ag-sec cyan"><div class="ag-h"><span class="ix">02</span><span class="t">Sense your team</span></div><a class="ag-item" href="#7"><span class="n">05</span><span>Who on my team has an active session right now, and what is each person working on?</span></a><a class="ag-item" href="#8"><span class="n">06</span><span>Summarize my teammates&#x27; sessions from the last day — one short paragraph each.</span></a><a class="ag-item" href="#9"><span class="n">07</span><span>Show me live activity across the team in the last hour — who&#x27;s streaming, on what branch.</span></a><a class="ag-item" href="#10"><span class="n">08</span><span>Find a teammate&#x27;s sessions and summarize what they&#x27;ve been focused on this week, so I can sync before we talk.</span></a></div><div class="ag-sec purple"><div class="ag-h"><span class="ix">03</span><span class="t">Narrate the story</span></div><a class="ag-item" href="#11"><span class="n">09</span><span>Trace the story of how this project came to be from my session history. Highlight the key decisions and turning points — and script it so the result is deterministic.</span></a><a class="ag-item" href="#12"><span class="n">10</span><span>Compile every session related to &lt;topic&gt; and narrate the arc, start to finish.</span></a><a class="ag-item" href="#13"><span class="n">11</span><span>Write me a standup update from today&#x27;s sessions: what I did, what&#x27;s blocked, and what&#x27;s next.</span></a></div><div class="ag-sec green"><div class="ag-h"><span class="ix">04</span><span class="t">Coach yourself</span></div><a class="ag-item" href="#14"><span class="n">12</span><span>Analyze my sessions from the last month and give me honest feedback on my prompt engineering — what I do well, and what I could do better.</span></a><a class="ag-item" href="#15"><span class="n">13</span><span>Find where my sessions tend to stall, loop, or repeat work. What are my recurring failure patterns?</span></a><a class="ag-item" href="#16"><span class="n">14</span><span>What direction has my work been pointing lately? Cluster my recent sessions by theme.</span></a></div><div class="ag-sec orange"><div class="ag-h"><span class="ix">05</span><span class="t">Recall anything</span></div><a class="ag-item" href="#17"><span class="n">15</span><span>Find the last time I solved &lt;problem&gt;, and show me exactly how I did it.</span></a><a class="ag-item" href="#18"><span class="n">16</span><span>Did I ever set up &lt;X&gt;? Locate the session and pull out the precise commands.</span></a><a class="ag-item" href="#19"><span class="n">17</span><span>Search my sessions for &lt;topic&gt; and list every session that touched it, newest first.</span></a></div><div class="ag-sec red"><div class="ag-h"><span class="ix">06</span><span class="t">Ground your agent</span></div><a class="ag-item" href="#20"><span class="n">18</span><span>Before you start, query OpenStory for prior context on this project and pick up where the last session left off.</span></a><a class="ag-item" href="#21"><span class="n">19</span><span>Use OpenStory to check whether we&#x27;ve hit this error before — and what fixed it — before debugging from scratch.</span></a><a class="ag-item" href="#22"><span class="n">20</span><span>Watch the work happening on &lt;branch&gt; through OpenStory and summarize it for me as it streams.</span></a></div></div>

---

<!-- _class: blue -->

<div class="crumb"><span class="ix">01.1</span><span>Know your own work</span><span class="tag real">real · live data</span></div>
<div class="q"><span class="qn">01</span>Summarize everything I&#x27;ve worked on this week across all my projects — group it by project and tell me what actually shipped.</div>
<div class="viz"><div class="vstats"><div class="st"><div class="n">25</div><div class="l">sessions</div></div><div class="st"><div class="n">4</div><div class="l">projects</div></div><div class="st"><div class="n">13.2K</div><div class="l">events</div></div></div><table class="vtable"><thead><tr><th>Project</th><th class="r">Sess</th><th>Activity</th><th>What shipped</th></tr></thead><tbody><tr><td>a-project</td><td class="r">26</td><td><span class="cellbar"><span style="width:100%"></span></span>9.3K</td><td>store fix · codex view parity · replay tests</td></tr><tr><td>a-website</td><td class="r">10</td><td><span class="cellbar"><span style="width:29%"></span></span>2.7K</td><td>starter-prompt library page</td></tr><tr><td>a-deploy</td><td class="r">8</td><td><span class="cellbar"><span style="width:10%"></span></span>0.9K</td><td>distributed NATS leaf hardening</td></tr><tr><td>research-spike</td><td class="r">1</td><td><span class="cellbar"><span style="width:3%"></span></span>0.3K</td><td>one-off analysis</td></tr></tbody></table><div class="vnote"><b>6 commits</b> landed this week · zero regressions on golden replays.</div><div class="src">GET /api/sessions → group by project  +  git log --since</div></div>

---

<!-- _class: blue -->

<div class="crumb"><span class="ix">01.2</span><span>Know your own work</span><span class="tag real">real · live data</span></div>
<div class="q"><span class="qn">02</span>What have my agent sessions cost me? Give me the total spend, and a tokens-per-day timeline.</div>
<div class="viz"><div class="vstats"><div class="st"><div class="n">$615</div><div class="l">spent · 7 days</div></div><div class="st ac"><div class="n">$3,141</div><div class="l">saved by cache</div></div><div class="st ac"><div class="n">84%</div><div class="l">off retail</div></div></div><div><div class="vtitle">Cost per day</div><div class="vbars"><div class="vbar"><span class="bl">Jun 13</span><span class="bt"><span class="bf" style="width:100%"></span></span><span class="bv">$304</span></div><div class="vbar"><span class="bl">Jun 14</span><span class="bt"><span class="bf" style="width:40%"></span></span><span class="bv">$122</span></div><div class="vbar"><span class="bl">Jun 17</span><span class="bt"><span class="bf" style="width:29%"></span></span><span class="bv">$89</span></div><div class="vbar"><span class="bl">Jun 16</span><span class="bt"><span class="bf" style="width:14%"></span></span><span class="bv">$44</span></div><div class="vbar"><span class="bl">Jun 12</span><span class="bt"><span class="bf" style="width:4%"></span></span><span class="bv">$12</span></div><div class="vbar"><span class="bl">Jun 18</span><span class="bt"><span class="bf" style="width:4%"></span></span><span class="bv">$11</span></div></div></div><div class="vnote">Biggest single call: <b>642K</b> prompt tokens.</div><div class="src">python3 scripts/token_usage.py --days 7 --by-day</div></div>

---

<!-- _class: blue -->

<div class="crumb"><span class="ix">01.3</span><span>Know your own work</span><span class="tag illus">illustrative</span></div>
<div class="q"><span class="qn">03</span>Before I share my session history with anyone, scan it for anything sensitive — secrets, API keys, private details, or anything I&#x27;d be embarrassed to expose.</div>
<div class="viz"><div class="vstats"><div class="st"><div class="n">~19.6K</div><div class="l">events scanned</div></div><div class="st"><div class="n">119</div><div class="l">sessions</div></div><div class="st ac"><div class="n">2</div><div class="l">high-severity</div></div></div><table class="vtable"><thead><tr><th>Category</th><th>Exposure</th><th class="r">Severity</th></tr></thead><tbody><tr><td>Personal fs paths</td><td>common</td><td class="r"><span class="chip-sev lo">Low</span></td></tr><tr><td>Private IPs / hosts</td><td>some</td><td class="r"><span class="chip-sev med">Medium</span></td></tr><tr><td>Email addresses</td><td>some</td><td class="r"><span class="chip-sev med">Medium</span></td></tr><tr><td>Inline credentials</td><td>a few</td><td class="r"><span class="chip-sev hi">High</span></td></tr><tr><td>Named secret assigns</td><td>rare</td><td class="r"><span class="chip-sev hi">High</span></td></tr></tbody></table><div class="vnote warn">Sample (redacted): <b>password=[REDACTED]</b> · no live API keys (sk- / ghp_ / AKIA) found. Scrub the High-severity sessions before sharing.</div><div class="src">regex sweep over event payloads (pattern set from scripts/scrub_check.py) — values never printed</div></div>

---

<!-- _class: blue -->

<div class="crumb"><span class="ix">01.4</span><span>Know your own work</span><span class="tag real">real · live data</span></div>
<div class="q"><span class="qn">04</span>Which tools and commands do I rely on most, and where does my time actually go?</div>
<div class="viz"><div><div class="vtitle">Where time goes</div><div class="vbars"><div class="vbar"><span class="bl">Run / shell</span><span class="bt"><span class="bf" style="width:53%"></span></span><span class="bv">53%</span></div><div class="vbar"><span class="bl">Read code</span><span class="bt"><span class="bf" style="width:21%"></span></span><span class="bv">21%</span></div><div class="vbar"><span class="bl">Write code</span><span class="bt"><span class="bf" style="width:18%"></span></span><span class="bv">18%</span></div><div class="vbar"><span class="bl">Plan / track</span><span class="bt"><span class="bf" style="width:3%"></span></span><span class="bv">3%</span></div><div class="vbar"><span class="bl">Research</span><span class="bt"><span class="bf" style="width:1%"></span></span><span class="bv">1%</span></div></div></div><div class="vchips"><span class="cl">Top commands</span><span class="vchip">grep · 127</span><span class="vchip">git · 46</span><span class="vchip">ssh · 31</span><span class="vchip">python3 · 27</span><span class="vchip">curl · 24</span></div><div class="vnote"><b>Read:Write 1.2:1</b> · most-used tool: Bash (53% of all calls).</div><div class="src">SQL: group message.assistant.tool_use by tool name + bash first-word</div></div>

---

<!-- _class: cyan -->

<div class="crumb"><span class="ix">02.1</span><span>Sense your team</span><span class="tag illus">illustrative</span></div>
<div class="q"><span class="qn">05</span>Who on my team has an active session right now, and what is each person working on?</div>
<div class="viz"><div class="vstats"><div class="st ac"><div class="n">2</div><div class="l">live now</div></div><div class="st"><div class="n">1</div><div class="l">idle &lt; 1h</div></div><div class="st"><div class="n">4</div><div class="l">federated hosts</div></div></div><table class="vtable"><thead><tr><th></th><th>Teammate</th><th>Project</th><th>Branch</th><th class="r">Idle</th></tr></thead><tbody><tr><td><span class="dot on">●</span></td><td>Teammate A</td><td>a-project · UI</td><td>master</td><td class="r">&lt;1m</td></tr><tr><td><span class="dot on">●</span></td><td>Teammate B</td><td>a-project</td><td>feat/macos-watcher</td><td class="r">2m</td></tr><tr><td><span class="dot off">○</span></td><td>Teammate C</td><td>project-x</td><td>feat/federation</td><td class="r">1h 42m</td></tr></tbody></table><div class="src">GET /api/sessions → group by host/user, keep last_event within window</div></div>

---

<!-- _class: cyan -->

<div class="crumb"><span class="ix">02.2</span><span>Sense your team</span><span class="tag real">real · live data</span></div>
<div class="q"><span class="qn">06</span>Summarize my teammates&#x27; sessions from the last day — one short paragraph each.</div>
<div class="viz"><div class="vcards"><div class="vcard"><div class="ch">Teammate A<span class="cs">6 sessions · ~620 events</span></div><div class="ct">UI prototype + agentic-team repo. Pulled latest on master, stood up a Dockerised NATS server, opened a thread to find dev-loop friction. Branches: master, main, sprint-9/dev-loop-rebuild.</div></div><div class="vcard"><div class="ch">Teammate B<span class="cs">1 session · 296 events</span></div><div class="ct">One deep codex run on project-x; no feature branch.</div></div></div><div class="src">GET /api/sessions → filter teammate + last_event within 24h</div></div>

---

<!-- _class: cyan -->

<div class="crumb"><span class="ix">02.3</span><span>Sense your team</span><span class="tag illus">illustrative</span></div>
<div class="q"><span class="qn">07</span>Show me live activity across the team in the last hour — who&#x27;s streaming, on what branch.</div>
<div class="viz"><div class="vstats"><div class="st ac"><div class="n">2</div><div class="l">streaming</div></div><div class="st"><div class="n">2</div><div class="l">branches active</div></div></div><table class="vtable"><thead><tr><th></th><th>Teammate</th><th>Project</th><th>Branch</th><th class="r">Events</th></tr></thead><tbody><tr><td><span class="dot on">●</span></td><td>Teammate B</td><td>a-project</td><td>feat/macos-watcher</td><td class="r">17</td></tr><tr><td><span class="dot on">●</span></td><td>Teammate C</td><td>project-x</td><td>chore/idle-watcher</td><td class="r">306</td></tr></tbody></table><div class="src">GET /api/sessions → keep last_event within 60 min, status=ongoing</div></div>

---

<!-- _class: cyan -->

<div class="crumb"><span class="ix">02.4</span><span>Sense your team</span><span class="tag real">real · live data</span></div>
<div class="q"><span class="qn">08</span>Find a teammate&#x27;s sessions and summarize what they&#x27;ve been focused on this week, so I can sync before we talk.</div>
<div class="viz"><div class="vstats"><div class="st"><div class="n">62</div><div class="l">sessions · this wk</div></div><div class="st"><div class="n">~10.3K</div><div class="l">events</div></div></div><div><div class="vtitle">Top projects</div><div class="vbars"><div class="vbar"><span class="bl">a-project · UI</span><span class="bt"><span class="bf" style="width:100%"></span></span><span class="bv">25</span></div><div class="vbar"><span class="bl">agentic-team</span><span class="bt"><span class="bf" style="width:60%"></span></span><span class="bv">15</span></div><div class="vbar"><span class="bl">a-project</span><span class="bt"><span class="bf" style="width:52%"></span></span><span class="bv">13</span></div></div></div><div class="vchips"><span class="cl">Branches</span><span class="vchip">master (11)</span><span class="vchip">main (11)</span><span class="vchip">sprint-1/wire-live-data (9)</span></div><div class="vnote"><b>Themes:</b> live-data wiring · dev-loop friction · sprint-9 rebuild. <b>Talk about:</b> the sprint-9 rebuild — their heaviest thread.</div><div class="src">GET /api/sessions → filter user; aggregate project / branch / events</div></div>

---

<!-- _class: purple -->

<div class="crumb"><span class="ix">03.1</span><span>Narrate the story</span><span class="tag real">real · live data</span></div>
<div class="q"><span class="qn">09</span>Trace the story of how this project came to be from my session history. Highlight the key decisions and turning points — and script it so the result is deterministic.</div>
<div class="viz"><div class="vtl"><div class="ev"><div class="d">2026-03-07</div><div class="t">First exploratory sessions</div><div class="nt">mapped the Rust + React shape before any code</div></div><div class="ev"><div class="d">2026-03-21</div><div class="t">Initial commit</div><div class="nt">the project formally begins</div></div><div class="ev"><div class="d">2026-03-24</div><div class="t">Multi-agent + first deploy</div><div class="nt">one agent → many; first live deploy</div></div><div class="ev"><div class="d">2026-03-31</div><div class="t">Vector search → SQLite FTS</div><div class="nt">cut a heavy dependency — “your data is yours”</div></div><div class="ev"><div class="d">2026-04-05</div><div class="t">Five-layer pipeline + Story tab</div><div class="nt">events → turns → sentences locks in</div></div><div class="ev"><div class="d">2026-06-11</div><div class="t">Security audit · 6 CVEs fixed</div><div class="nt">production-readiness gate</div></div><div class="ev"><div class="d">2026-06-13</div><div class="t">Federation validated</div><div class="nt">single-machine → multi-machine streaming</div></div></div><div class="tlarc">A read-only file watcher → a federated, multi-agent store — every step preserving “observe, never interfere.”</div><div class="src">git log per milestone + sessions table (deterministic re-run)</div></div>

---

<!-- _class: purple -->

<div class="crumb"><span class="ix">03.2</span><span>Narrate the story</span><span class="tag real">real · live data</span></div>
<div class="q"><span class="qn">10</span>Compile every session related to &lt;topic&gt; and narrate the arc, start to finish.</div>
<div class="viz"><div class="vstats"><div class="st"><div class="n">63</div><div class="l">sessions</div></div><div class="st"><div class="n">6 days</div><div class="l">Jun 13–19</div></div></div><div class="vtl"><div class="ev"><div class="d">Jun 13</div><div class="t">Spike</div><div class="nt">“get our networks to talk” → a routing design report</div></div><div class="ev"><div class="d">Jun 13</div><div class="t">Design</div><div class="nt">agents map host-in-subject routing</div></div><div class="ev"><div class="d">Jun 14</div><div class="t">Build</div><div class="nt">host encoded in event subject; node auto-discovers the hub</div></div><div class="ev"><div class="d">Jun 16</div><div class="t">Polish</div><div class="nt">UI copy: “configured” vs “live sources”</div></div><div class="ev"><div class="d">Jun 18</div><div class="t">Reflect</div><div class="nt">state-of-work reflection consolidates the arc</div></div></div><div class="tlarc">Federation validated end-to-end; one binary now joins a shared hub.</div><div class="src">GET /api/search?q=&lt;topic&gt;  +  git log</div></div>

---

<!-- _class: purple -->

<div class="crumb"><span class="ix">03.3</span><span>Narrate the story</span><span class="tag real">real · live data</span></div>
<div class="q"><span class="qn">11</span>Write me a standup update from today&#x27;s sessions: what I did, what&#x27;s blocked, and what&#x27;s next.</div>
<div class="viz"><div class="vgroups"><div class="vgroup"><div class="gh">Did</div><ul><li>State-of-work reflection across git + the store</li><li>Triaged a big batch of WIP into shippable value</li><li>Reconciled a second agent&#x27;s parallel work</li><li>Planned an extraction, executed it, opened a PR</li></ul></div><div class="vgroup warn"><div class="gh">Blocked</div><ul><li>Waiting on cross-device data confirmation</li></ul></div><div class="vgroup"><div class="gh">Next</div><ul><li>Finish the extraction</li><li>Land the open PR after review</li></ul></div></div><div class="vnote">1 session · 44 turns · top tools: <b>tool_use 211</b> · file.snapshot 70 · 24 errors handled.</div><div class="src">scripts/sessionstory.py &lt;id&gt; (today&#x27;s sessions)</div></div>

---

<!-- _class: green -->

<div class="crumb"><span class="ix">04.1</span><span>Coach yourself</span><span class="tag real">real · live data</span></div>
<div class="q"><span class="qn">12</span>Analyze my sessions from the last month and give me honest feedback on my prompt engineering — what I do well, and what I could do better.</div>
<div class="viz"><div class="vstats"><div class="st"><div class="n">424</div><div class="l">sessions · 30d</div></div><div class="st"><div class="n">835</div><div class="l">prompts</div></div></div><table class="vtable"><thead><tr><th>Metric</th><th class="r">Value</th><th>Read</th></tr></thead><tbody><tr><td>Median prompt length</td><td class="r">161c</td><td>terse by default</td></tr><tr><td>Long prompts (&gt;600c)</td><td class="r">35%</td><td>spec-grade briefs</td></tr><tr><td>Short prompts (&lt;80c)</td><td class="r">38%</td><td>one-liners</td></tr><tr><td>One-shot sessions</td><td class="r">83%</td><td>fire-and-forget</td></tr><tr><td>Avg turns / session</td><td class="r">10.4</td><td>runs long unattended</td></tr></tbody></table><div class="vnote"><b>Strength:</b> 35% are detailed briefs that set up 10+ autonomous turns. <b>Grow:</b> the 38% of &lt;80-char prompts have no follow-up to course-correct under an 83% one-shot rate — invest the brief up front.</div><div class="src">SQL over prompt lengths + turns-per-session</div></div>

---

<!-- _class: green -->

<div class="crumb"><span class="ix">04.2</span><span>Coach yourself</span><span class="tag real">real · live data</span></div>
<div class="q"><span class="qn">13</span>Find where my sessions tend to stall, loop, or repeat work. What are my recurring failure patterns?</div>
<div class="viz"><table class="vtable"><thead><tr><th>Signal</th><th class="r">Rate</th><th>Meaning</th></tr></thead><tbody><tr><td>3+ identical tool calls / session</td><td class="r">27%</td><td>loop / thrash</td></tr><tr><td>File edited 5+× in a session</td><td class="r">46×</td><td>rework churn</td></tr><tr><td>Tool-result errors</td><td class="r">1.7%</td><td>clean execution</td></tr><tr><td>Sessions hitting an error</td><td class="r">3%</td><td>rarely blocked</td></tr><tr><td>Sessions hitting compaction</td><td class="r">1%</td><td>context bounded</td></tr></tbody></table><div class="vnote warn"><b>#1 failure mode: edit-thrash</b> — 89 Edit loops vs 28 Bash. Fix: read fully, edit once. Execution is healthy; the waste is iteration churn.</div><div class="src">SQL: repeated tool-call detection + error / compaction counts</div></div>

---

<!-- _class: green -->

<div class="crumb"><span class="ix">04.3</span><span>Coach yourself</span><span class="tag real">real · live data</span></div>
<div class="q"><span class="qn">14</span>What direction has my work been pointing lately? Cluster my recent sessions by theme.</div>
<div class="viz"><div><div class="vtitle">Themes · last 30 days</div><div class="vbars"><div class="vbar"><span class="bl">Docs / research</span><span class="bt"><span class="bf" style="width:40%"></span></span><span class="bv">40%</span></div><div class="vbar"><span class="bl">Deploy / infra</span><span class="bt"><span class="bf" style="width:24%"></span></span><span class="bv">24%</span></div><div class="vbar"><span class="bl">Security / audit</span><span class="bt"><span class="bf" style="width:12%"></span></span><span class="bv">12%</span></div><div class="vbar"><span class="bl">Data pipeline</span><span class="bt"><span class="bf" style="width:9%"></span></span><span class="bv">9%</span></div><div class="vbar"><span class="bl">Testing / E2E</span><span class="bt"><span class="bf" style="width:7%"></span></span><span class="bv">7%</span></div><div class="vbar"><span class="bl">UI / frontend</span><span class="bt"><span class="bf" style="width:6%"></span></span><span class="bv">6%</span></div></div></div><div class="vnote"><b>Through-line:</b> shifted from building features to shipping &amp; explaining — 64% of effort is docs + deploy/infra.</div><div class="src">keyword-bucketing of prompt text + tool histogram</div></div>

---

<!-- _class: orange -->

<div class="crumb"><span class="ix">05.1</span><span>Recall anything</span><span class="tag real">real · live data</span></div>
<div class="q"><span class="qn">15</span>Find the last time I solved &lt;problem&gt;, and show me exactly how I did it.</div>
<div class="viz"><dl class="vmeta"><dt>Problem</dt><dd>NATS JetStream stream creation crashes on first boot</dd><dt>Last solved</dt><dd>2026-06-13 · a-project</dd><dt>Root cause</dt><dd>store quota too small (max_file &lt; ~1.3GB)</dd></dl><div class="vsteps"><div class="vstep"><code>cp nats.conf nats.conf.bak</code></div><div class="vstep"><code>set jetstream { store_dir: &lt;path&gt;, max_file: 4GB }</code></div><div class="vstep"><code>nats-server -c nats.conf -t          # validate</code></div><div class="vstep"><code>kill -HUP $(pgrep -f nats-server)    # hot reload, no restart</code></div></div><div class="vnote"><b>Verified:</b> curl …:8222/leafz → healthy stream. Also hit 2026-06-10; earliest 2026-04-30.</div><div class="src">GET /api/search?q=max_file+stream+crash → tool_use commands</div></div>

---

<!-- _class: orange -->

<div class="crumb"><span class="ix">05.2</span><span>Recall anything</span><span class="tag real">real · live data</span></div>
<div class="q"><span class="qn">16</span>Did I ever set up &lt;X&gt;? Locate the session and pull out the precise commands.</div>
<div class="viz"><dl class="vmeta"><dt>Set up</dt><dd>NATS leaf-node federation → YES</dd><dt>Where</dt><dd>2026-06-13 · a-project</dd></dl><div class="vcode">$ cp nats.conf nats.conf.bak
$ source .env.federation              # NATS_LEAF_URL from env, not git
$ printf &#x27;leafnodes { remotes [ {url:&quot;%s&quot;} ] }\n&#x27; &quot;$NATS_LEAF_URL&quot; \
      &gt; deploy/leaf-remotes.generated.conf
$ nats-server -c nats.conf -t         # validate
$ kill -HUP $(pgrep -f nats-server)   # reload, no downtime</div><div class="vnote"><b>Proof:</b> curl …:8222/leafz → leaf connection live.</div><div class="src">GET /api/search?q=nats+leaf+hub → Bash commands</div></div>

---

<!-- _class: orange -->

<div class="crumb"><span class="ix">05.3</span><span>Recall anything</span><span class="tag real">real · live data</span></div>
<div class="q"><span class="qn">17</span>Search my sessions for &lt;topic&gt; and list every session that touched it, newest first.</div>
<div class="viz"><div class="vstats"><div class="st"><div class="n">8</div><div class="l">sessions</div></div><div class="st"><div class="n">894</div><div class="l">mentions</div></div></div><div class="vlist"><div class="li"><span class="ld">Jun 19</span><span>session &lt;id&gt; · a-project</span><span class="lm">2 hits</span></div><div class="li"><span class="ld">Jun 18</span><span>session &lt;id&gt; · a-project</span><span class="lm">4 hits</span></div><div class="li"><span class="ld">Jun 17</span><span>session &lt;id&gt; · a-project</span><span class="lm">1 hit</span></div><div class="li"><span class="ld">Jun 13</span><span>session &lt;id&gt; · a-project</span><span class="lm">4 hits</span></div><div class="li"><span class="ld">Jun 12</span><span>session &lt;id&gt; · a-project</span><span class="lm">5 hits</span></div></div><div class="vnote">Newest: wiring the NATS leaf over the tailnet so a remote agent streams home. (+3 older)</div><div class="src">GET /api/search?q=&lt;topic&gt; (grouped by session, newest first)</div></div>

---

<!-- _class: red -->

<div class="crumb"><span class="ix">06.1</span><span>Ground your agent</span><span class="tag real">real · live data</span></div>
<div class="q"><span class="qn">18</span>Before you start, query OpenStory for prior context on this project and pick up where the last session left off.</div>
<div class="viz"><dl class="vmeta"><dt>Last session</dt><dd>2026-06-19 · branch chore/x · 312 events</dd><dt>Worked on</dt><dd>mined the store for common prompts → built a copy-paste prompt library + live demos</dd><dt>Tools</dt><dd>Bash×36 · Edit×23 · Read×5 · Write×4 · Agent×4</dd><dt>Open threads</dt><dd>wire demos to live data · keep copy public-safe</dd></dl><div class="vnote"><b>Resume here →</b> 6 template subagents were mid-flight; collect results and wire the live examples into the page.</div><div class="src">GET /api/sessions (newest) + /api/sessions/{id}/records (or sessionstory.py {id} --unfinished)</div></div>

---

<!-- _class: red -->

<div class="crumb"><span class="ix">06.2</span><span>Ground your agent</span><span class="tag real">real · live data</span></div>
<div class="q"><span class="qn">19</span>Use OpenStory to check whether we&#x27;ve hit this error before — and what fixed it — before debugging from scratch.</div>
<div class="viz"><div class="vstats"><div class="st ac"><div class="n">yes</div><div class="l">seen before</div></div><div class="st"><div class="n">113</div><div class="l">bind / conn errors</div></div><div class="st"><div class="n">174×</div><div class="l">lsof+kill in record</div></div></div><div class="vcode">lsof -ti:3002,5173,4222     # find the holders
kill &lt;pids&gt;; sleep 1        # release them, then re-boot</div><div class="vnote">A leftover server / Docker stack holds the port at boot. <b>Free the ports first — don&#x27;t re-debug the bind.</b> (docker-mode: compose down)</div><div class="src">GET /api/search?q=Address+already+in+use  + system.error tally</div></div>

---

<!-- _class: red -->

<div class="crumb"><span class="ix">06.3</span><span>Ground your agent</span><span class="tag illus">illustrative</span></div>
<div class="q"><span class="qn">20</span>Watch the work happening on &lt;branch&gt; through OpenStory and summarize it for me as it streams.</div>
<div class="viz"><div class="vfeed"><div class="fr"><span class="ft">01:14:39</span><span class="fk">Agent</span><span class="fx">fan out report-template subagent</span></div><div class="fr"><span class="ft">01:14:10</span><span class="fk">Agent</span><span class="fx">fan out next subagent</span></div><div class="fr"><span class="ft">23:27:07</span><span class="fk">Bash</span><span class="fx">cd rs &amp;&amp; cargo test …  (verifying watcher)</span></div><div class="fr"><span class="ft">23:26:59</span><span class="fk">Edit</span><span class="fx">rs/src/watcher.rs  (4 rapid edits)</span></div><div class="fr"><span class="ft">23:25:16</span><span class="fk">Bash</span><span class="fx">write prompt-library draft</span></div></div><div class="vstats"><div class="st"><div class="n">3</div><div class="l">sessions</div></div><div class="st"><div class="n">1,438</div><div class="l">events</div></div></div><div class="vnote">Replayed from the record (not a live socket). A true ticker → <b>subscribe_session</b> (WebSocket).</div><div class="src">SQL: recent tool_use for the branch. Live ticker → subscribe_session (WebSocket)</div></div>

---

<!-- _class: contents -->

## Or skip the prompt — just type the command

<p class="lede">The same reports ship as <strong>openstory-skills</strong>, a Claude Code plugin. Twelve slash commands, backed by the OpenStory MCP server.</p>
<div class="skills"><div class="skill blue"><span class="cmd">/cost</span><span class="sd">total spend, cache savings, tokens-per-day</span></div><div class="skill blue"><span class="cmd">/time</span><span class="sd">where time goes — by project, by hour</span></div><div class="skill blue"><span class="cmd">/tools</span><span class="sd">top tools &amp; commands; read-vs-write ratio</span></div><div class="skill blue"><span class="cmd">/scan</span><span class="sd">find secrets before you share your history</span></div><div class="skill cyan"><span class="cmd">/team</span><span class="sd">who&#x27;s active now &amp; what each teammate&#x27;s on</span></div><div class="skill purple"><span class="cmd">/arc</span><span class="sd">a project&#x27;s story as a deterministic timeline</span></div><div class="skill purple"><span class="cmd">/standup</span><span class="sd">today&#x27;s work → did · blocked · next</span></div><div class="skill green"><span class="cmd">/coach</span><span class="sd">honest feedback on your prompting &amp; patterns</span></div><div class="skill orange"><span class="cmd">/recall</span><span class="sd">find how you solved it before — exact commands</span></div><div class="skill orange"><span class="cmd">/recap</span><span class="sd">what you shipped lately, grouped by project</span></div><div class="skill red"><span class="cmd">/prime</span><span class="sd">pick up where the last session left off</span></div><div class="skill red"><span class="cmd">/watch</span><span class="sd">a live feed of a branch as it streams</span></div></div>
