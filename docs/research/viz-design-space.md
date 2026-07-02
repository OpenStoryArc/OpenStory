# Visualization Design Space for OpenStory

A catalog of the D3 shape space, a mapping from OpenStory's natural data shapes onto
those shapes, and an inventory of the interaction affordances a user can exercise.

**What already exists (do not re-propose):**
- Contribution heatmap (2D + 3D)
- Canvas tab with 6 modes: **Board** (circle-pack node board), **Sunburst**, **Treemap**,
  **Gantt** (lane-packed timeline), **Scatter** (events × output-tokens, log-log), **Flow**
  (bipartite tool→tool sankey).
- Tabs: Overview, Canvas, Heatmap, Story, Explore, Users, Admin, Ask.

**Stack:** React + D3 (`d3-shape`, `d3-hierarchy`, `d3-scale`, `d3-zoom`, `d3-brush`,
`d3-force`) + `@react-three/fiber`.

---

## A. D3 shape space

| Shape | d3 module | Geometry | Data shape it needs | Core or plugin |
|-------|-----------|----------|---------------------|----------------|
| Line | `d3-shape` (`line`) | polyline over x/y | ordered `[{x,y}]` series | core |
| Area | `d3-shape` (`area`) | filled band to baseline | ordered `[{x,y0,y1}]` | core |
| Stacked area | `d3-shape` (`stack`+`area`) | stacked filled bands | wide table, keyed series | core |
| Streamgraph | `d3-shape` (`stack.offset(wiggle)`) | centered stacked flow | time × category counts | core |
| Bar / column | `d3-scale` + rects | rectangles on band scale | `[{cat,value}]` | core (rects) |
| Radial bar | `d3-shape` (`arc`) + band-angle scale | bars wrapped to a ring | `[{cat,value}]` | core |
| Arc / pie / donut | `d3-shape` (`pie`,`arc`) | angular wedges | `[{cat,value}]` | core |
| Chord | `d3-chord` + `d3-shape` (`ribbon`) | ribbons inside a ring | square matrix `n×n` | core |
| Force-directed graph | `d3-force` | node/link physics layout | `{nodes[],links[]}` | core |
| Tree / dendrogram | `d3-hierarchy` (`tree`) | node-link hierarchy | rooted tree (parent→child) | core |
| Cluster | `d3-hierarchy` (`cluster`) | leaves aligned dendrogram | rooted tree | core |
| Circle-pack | `d3-hierarchy` (`pack`) | nested circles | rooted weighted tree | core |
| Icicle / partition | `d3-hierarchy` (`partition`) | nested rectangles (linear) | rooted weighted tree | core |
| Sunburst | `d3-hierarchy` (`partition`) + `arc` | nested rings (radial partition) | rooted weighted tree | core |
| Treemap | `d3-hierarchy` (`treemap`) | space-filling rectangles | rooted weighted tree | core |
| Sankey | `d3-sankey` | flow diagram, weighted links | `{nodes[],links[value]}` DAG | **plugin** |
| Arc-diagram | `d3-shape` (`linkHorizontal`/arc path) | 1D nodes + arcs above | linear nodes + edge list | core (hand-rolled) |
| Hexbin | `d3-hexbin` | hexagonal 2D density bins | `[{x,y}]` point cloud | **plugin** |
| Contour / density | `d3-contour` | isolines / heat field | `[{x,y}]` or grid | core (`d3-contour`) |
| Matrix / adjacency heatmap | `d3-scale` + rects | colored grid | `row × col × value` | core (rects) |
| Calendar heatmap | `d3-time` + rects | day-cells laid by week/weekday | `[{date,value}]` | core (rects) |
| Beeswarm | `d3-force` (`forceX`+collide) or dodge | jittered 1D dot dist | `[{value}]` per category | core |
| Ridgeline | `d3-shape` (`area`) + `d3-array` (bins/KDE) | stacked overlapping densities | many distributions | core |
| Parallel coordinates | `d3-scale` (many) + `line` | multi-axis polylines | rows over N dimensions | core |
| Radar / spider | `d3-shape` (`lineRadial`,`areaRadial`) | polygon over angular axes | `[{axis,value}]` per item | core |
| Voronoi / cells | `d3-delaunay` (`voronoi`) | space-partition polygons | `[{x,y}]` sites | core (`d3-delaunay`) |
| Voronoi treemap | `d3-voronoi-treemap` | weighted cell tessellation | rooted weighted tree | **plugin** |
| Bump chart | `d3-shape` (`line`,`curveBumpX`) | rank lines over periods | `item × period × rank` | core |
| Waffle | `d3-scale` + rect grid | unit-square proportion grid | `[{cat,value}]` | core (rects) |
| Slope / bump (2-period) | `d3-shape` (`line`) | connected before/after | `[{item,a,b}]` | core |
| Horizon chart | `d3-shape`(`area`) + banding | folded compact area bands | dense time series | core (hand-rolled) |
| Marimekko / mosaic | `d3-scale` + rects | variable-width stacked bars | 2-way table with margins | core (rects) |
| Gauge / arc-progress | `d3-shape` (`arc`) | partial ring | single scalar 0–1 | core |

---

## B. OpenStory data shape → feedable D3 shapes

| OpenStory data shape | Feeds these D3 shapes |
|----------------------|-----------------------|
| **Flat session rows** `{events, tokens, duration, agent, project, user, host, branch, status}` | beeswarm, parallel-coordinates, scatter*, radar, waffle, bar, histogram/hexbin, marimekko |
| **Session→subagent tree/graph** (`agent-*` children of a parent) | force-directed graph, tree/dendrogram, cluster, arc-diagram, circle-pack*, icicle, sunburst* |
| **Per-session event time-series** | line, area, streamgraph, horizon, ridgeline, calendar, bump |
| **Day-buckets (calendar)** | calendar-heatmap*, bump chart, streamgraph, ridgeline |
| **agent×project / user×project matrix** | adjacency heatmap, chord, marimekko, force graph, sankey |
| **tool→tool transition counts** | sankey* (exists as Flow), chord, arc-diagram, adjacency heatmap |
| **token / event distributions (long-tail, ~6 decades)** | beeswarm, ridgeline, histogram (log bins), violin, ECDF/line |
| **turn phases / eval-apply within a session** | icicle, gantt*, arc-diagram, horizon |

`*` = already realized in an existing OpenStory view (heatmap or a Canvas mode).

**Under-tapped shapes** (rich in the store, thin in the UI):
1. **Session→subagent delegation graph** — the store has ~623 subagents linked to ~805 parents,
   yet no view renders the *delegation topology* itself. Only Board/Sunburst nest them by group,
   not by spawn lineage.
2. **agent×project (and user×project) matrix** — a first-class cross-tab exists in the analytics
   layer but is never shown as an adjacency/chord/marimekko surface.
3. **Distributions over the flat rows** — tokens/events/duration are 6-decade long-tailed; the only
   distribution surface is Scatter (2 vars, no per-agent density). Beeswarm/ridgeline/parallel-coords
   are unused.

---

## C. Interaction space inventory (per view)

Affordance taxonomy: **navigate · click · hover · drag · zoom · pan · brush · select · filter · sort · expand · drill · toggle**.

### Overview
- hover metric cards → tooltip detail; click card → navigate to filtered list; toggle time-window;
  sort session list by column; click session row → drill to Story; filter by agent/project chip.

### Canvas (shared shell)
- toggle mode (board/sunburst/treemap/gantt/scatter/flow); toggle group-by
  (project/agent/user/host); toggle size metric (events vs tokens); zoom/pan the stage;
  click node → drill to session; hover → tooltip.
  - **Board:** click bubble → expand group→project→session; drag to reposition (force); zoom.
  - **Sunburst:** click ring segment → zoom-to-node (re-root); hover → breadcrumb; click center → zoom out.
  - **Treemap:** click cell → drill/re-root; hover → path + value; toggle size metric.
  - **Gantt:** brush time axis → filter window; hover bar → session summary; sort/pack lanes by group; pan timeline.
  - **Scatter:** brush region → select sessions; hover dot → session; toggle log/linear; click dot → drill.
  - **Flow:** select agent; hover link → transition count; hover node → tool totals; drag node to reorder.

### Heatmap (2D + 3D contribution)
- hover cell → day/count tooltip; click cell/day → drill to that day's sessions; toggle 2D/3D;
  drag-orbit (3D camera); zoom; toggle metric (events/tokens/sessions); filter by agent/user.

### Story
- select session; navigate turn-by-turn; expand tool_use/tool_result; toggle raw/rendered;
  click file/tool reference → drill; scroll timeline; filter event subtypes.

### Explore
- filter by project/agent/user/status; sort columns; select session → open Conversation;
  click event → drill to record; search-in-view; toggle grouping.

### Users
- select user/principal; hover → fleet stats; click → filter sessions by principal;
  toggle person vs principal grouping; sort by activity.

### Admin
- toggle person-cluster assignments; select principal; drag to reassign; edit matchers;
  filter unassigned events.

### Ask
- type query (navigate/query); click a cited session → drill to Story/Explore;
  toggle result grouping; select suggested follow-up.

**Cross-cutting affordances** present nearly everywhere: `navigate` (route change), `hover`
(tooltip), `click`→`drill` (to Story/Explore), `filter` (agent/project/user/status chips),
`toggle` (metric / grouping / dimensionality), `brush`/`select` (time or region → subset).
