# Patterns

What we've learned works — and what doesn't — from building Open Story.

## Patterns that work

### Boundary table BDD

Write the full spec as a data table before implementing. Cover: empty, single, many, overflow, tie-breaking, error cases. The table IS the spec.

```typescript
const CASES: [string, Input, Expected][] = [
  ["empty input",    [],    []],
  ["single item",    [x],   [y]],
  ["overflow",       [...],  [...]],
  ["tie-breaking",   [...],  [...]],
];

it.each(CASES)("%s", (_, input, expected) => {
  scenario(
    () => input,
    (x) => transform(x),
    (result) => expect(result).toEqual(expected),
  );
});
```

Every edge case in one place. Readable, compact, catches what narrative tests miss.

### scenario(given, when, then)

Pure pipeline: data flows explicitly through three functions. No shared mutable state, no hidden closures. Every test follows this shape. It makes tests scannable and prevents setup pollution between tests.

### Prototype in scripts, implement in production code

Query real data with a script. Validate the data model. Print human-readable output. Then implement in production code. The prototype catches wrong assumptions before you invest in UI.

This prevented us from building a tree UI for data that isn't a tree. It revealed that most sessions are subagent spawns. It showed that truncation saves almost nothing.

### Shared rendering, separate data ownership

`EventCard` is shared between Live and Explore. Same card, different data source. Share the presentation, never the data fetching.

### Single-pass index building

Build all indexes (turns, files, tools, agents, errors) in one iteration. No second passes, no lookups during construction. O(n) and cache-friendly.

### Noise filtering at fetch time

Strip non-displayable record types (token_usage, file_snapshot, session_meta) immediately after fetch. Filter once, not on every render.

### File hint propagation

To syntax-highlight Read tool results, the file path comes from the parent tool_call. Instead of Map lookups, track `lastToolFilePath` as you iterate — set on tool_call, consumed on tool_result, cleared after. Zero overhead.

### Pure functions as the unit of work

Every feature starts as a pure function with a boundary table. Components compose those functions. Test the functions, trust the composition.

---

## Anti-patterns to avoid

### Building before looking at the data

We built a tree abstraction, then discovered the data is a linked list. Query real data first. Write a script, print the shape, understand the distribution. Ten minutes of analysis saves hours of wrong implementation.

### Merging data from different sources into one view

WebSocket data is live and ephemeral. REST data is durable and complete. Merging them creates a view that's partially both and fully neither. Sessions appear but have no events. Formats mismatch. Keep views honest about their data source.

### Premature abstractions

We wrote a lazy-loading list abstraction for sessions with 500-2000 records. The data fits in memory, renders in milliseconds. The abstraction solved a problem that doesn't exist. When in doubt, skip the abstraction — three clear lines beat a clever helper.

### Truncation without measurement

A 2KB truncation threshold was set without measuring payloads. Analysis showed it affected 3% of records and saved less than 1MB total. The truncation metadata sometimes made payloads larger. Measure first, then decide.

### Assuming structure from field names

A field called `parent_uuid` suggests a tree. In practice, it creates a sequential chain where each event points to the previous one. Check the actual data shape — depth distributions, branching factors, real examples.

### Inline data analysis

Running ad-hoc Python one-liners in the shell produces results that vanish, can't be reviewed, and break on Windows. Write scripts with test flags, argparse, and clear output. Scripts are artifacts — they tell the story of how you learned what you know.
