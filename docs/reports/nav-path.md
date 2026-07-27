# Full click-parity nav survey

_scripts/nav_path.mjs — ActionGraph edges + canvas modes + day._

**29/29 landed**

| kind | path | ok | hash |
|---|---|:--:|---|
| edge | person→session | ✅ | `#/explore?user=max` |
| edge | project→session | ✅ | `#/explore?project=OpenStory` |
| edge | session→subagent | ✅ | `#/explore/019fa1ba-47ad-77a2-a0b8-63f069d46d77/graph` |
| edge | session→turn | ✅ | `#/story/019fa1ba-47ad-77a2-a0b8-63f069d46d77` |
| edge | session→plan | ✅ | `#/explore/019fa1ba-47ad-77a2-a0b8-63f069d46d77/plans` |
| edge | session→event | ✅ | `#/explore/019fa1ba-47ad-77a2-a0b8-63f069d46d77/event/9b32dd80-3166-528` |
| edge | turn→sentence | ✅ | `#/story/019fa1ba-47ad-77a2-a0b8-63f069d46d77/event/9b32dd80-3166-5281-` |
| edge | turn→event | ✅ | `#/explore/019fa1ba-47ad-77a2-a0b8-63f069d46d77/event/9b32dd80-3166-528` |
| edge | event→turn | ✅ | `#/story/019fa1ba-47ad-77a2-a0b8-63f069d46d77/event/9b32dd80-3166-5281-` |
| edge | subagent→session | ✅ | `#/explore/019fa1ba-47ad-77a2-a0b8-63f069d46d77` |
| edge | toolcall→result | ✅ | `#/explore/019fa1ba-47ad-77a2-a0b8-63f069d46d77/event/9b32dd80-3166-528` |
| edge | toolcall→file | ✅ | `#/search?q=App.tsx` |
| edge | file→session | ✅ | `#/search?q=App.tsx` |
| edge | error→event | ✅ | `#/explore/019fa1ba-47ad-77a2-a0b8-63f069d46d77/event/9b32dd80-3166-528` |
| edge | plan→turn | ✅ | `#/story/019fa1ba-47ad-77a2-a0b8-63f069d46d77/event/9b32dd80-3166-5281-` |
| multi | event→sentence | ✅ | `#/story/019fa1ba-47ad-77a2-a0b8-63f069d46d77/event/9b32dd80-3166-5281-` |
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
| expand | →expand | ✅ | `#/story/019fa1ba-47ad-77a2-a0b8-63f069d46d77/event/9b32dd80-3166-5281-` |
