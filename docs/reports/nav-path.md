# Full click-parity nav survey

_scripts/nav_path.mjs — ActionGraph edges + canvas modes + day._

**29/29 landed**

| kind | path | ok | hash |
|---|---|:--:|---|
| edge | person→session | ✅ | `#/explore?user=max` |
| edge | project→session | ✅ | `#/explore?project=OpenStory` |
| edge | session→subagent | ✅ | `#/explore/019fa5a9-1860-7921-814d-1e13e6803d2c/graph` |
| edge | session→turn | ✅ | `#/story/019fa5a9-1860-7921-814d-1e13e6803d2c` |
| edge | session→plan | ✅ | `#/explore/019fa5a9-1860-7921-814d-1e13e6803d2c/plans` |
| edge | session→event | ✅ | `#/explore/019fa5a9-1860-7921-814d-1e13e6803d2c/event/ad650195-91ec-5a3` |
| edge | turn→sentence | ✅ | `#/story/019fa5a9-1860-7921-814d-1e13e6803d2c/event/ad650195-91ec-5a34-` |
| edge | turn→event | ✅ | `#/explore/019fa5a9-1860-7921-814d-1e13e6803d2c/event/ad650195-91ec-5a3` |
| edge | event→turn | ✅ | `#/story/019fa5a9-1860-7921-814d-1e13e6803d2c/event/ad650195-91ec-5a34-` |
| edge | subagent→session | ✅ | `#/explore/019fa5a9-1860-7921-814d-1e13e6803d2c` |
| edge | toolcall→result | ✅ | `#/explore/019fa5a9-1860-7921-814d-1e13e6803d2c/event/ad650195-91ec-5a3` |
| edge | toolcall→file | ✅ | `#/search?q=App.tsx` |
| edge | file→session | ✅ | `#/search?q=App.tsx` |
| edge | error→event | ✅ | `#/explore/019fa5a9-1860-7921-814d-1e13e6803d2c/event/ad650195-91ec-5a3` |
| edge | plan→turn | ✅ | `#/story/019fa5a9-1860-7921-814d-1e13e6803d2c/event/ad650195-91ec-5a34-` |
| multi | event→sentence | ✅ | `#/story/019fa5a9-1860-7921-814d-1e13e6803d2c/event/ad650195-91ec-5a34-` |
| multi | person→session | ✅ | `#/explore?user=max` |
| canvas | canvas:sunburst | ✅ | `#/canvas` |
| canvas | canvas:board | ✅ | `#/canvas` |
| canvas | canvas:treemap | ✅ | `#/canvas` |
| canvas | canvas:gantt | ✅ | `#/canvas` |
| canvas | canvas:scatter | ✅ | `#/canvas` |
| canvas | canvas:flow | ✅ | `#/canvas` |
| canvas | canvas:tool-adjacency | ✅ | `#/canvas` |
| canvas | canvas:agent-project | ✅ | `#/canvas` |
| canvas | canvas:durations | ✅ | `#/canvas` |
| canvas | canvas:heatmap | ✅ | `#/canvas` |
| day | →day | ✅ | `#/explore?day=2026-07-27` |
| expand | →expand | ✅ | `#/story/019fa5a9-1860-7921-814d-1e13e6803d2c/event/ad650195-91ec-5a34-` |
