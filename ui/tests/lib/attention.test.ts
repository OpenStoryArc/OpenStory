/**
 * Attention algebra — pure tree of what the mirror shows.
 */
import { describe, it, expect } from "vitest";
import {
  emptyAttention,
  foldIntent,
  foldControl,
  foldSteps,
  attentionSatisfies,
  attentionSummary,
  realizeIntent,
  materializeAttention,
} from "@/lib/attention";
import { interpretControl } from "@/lib/ui-control";
import { planNavigateTo } from "@/lib/nav-path";
import {
  scatterPaintFromBrush,
  type ScatterPoint,
} from "@/lib/sessions-scatter";

describe("foldIntent — denotational navigate_to", () => {
  const base = emptyAttention({ view: "live" });

  it("event → story attention with details", () => {
    const next = foldIntent(base, {
      kind: "event",
      id: "EVT-1",
      sessionId: "SES-1",
      details: true,
    });
    expect(next?.route).toMatchObject({
      view: "story",
      sessionId: "SES-1",
      eventId: "EVT-1",
      storyDetails: true,
    });
    expect(attentionSatisfies(next!, {
      kind: "event",
      id: "EVT-1",
      sessionId: "SES-1",
      details: true,
    })).toBe(true);
  });

  it("expandAll opens details + eval + events", () => {
    const next = foldIntent(base, {
      kind: "event",
      id: "EVT-1",
      sessionId: "SES-1",
      expandAll: true,
    });
    expect(next?.route).toMatchObject({
      storyDetails: true,
      storyEvalOpen: true,
      storyEventsOpen: true,
    });
  });

  // NOTE: per-apply-row expansion (applyOpen / storyApplyOpen) specs were
  // committed ahead of their implementation on the base branch — trimmed to
  // committed reality; they return with the apply-open feature itself.

  describe("when navigate_to expands CycleCard recursive agents", () => {
    it("should put agent session ids on storyAgentOpen Attention", () => {
      const next = foldIntent(base, {
        kind: "event",
        id: "EVT-1",
        sessionId: "SES-1",
        agentOpen: ["agent-abc123", "agent-def456"],
      });
      expect(next?.route).toMatchObject({
        view: "story",
        sessionId: "SES-1",
        eventId: "EVT-1",
        storyDetails: true,
        storyEvalOpen: true,
        storyAgentOpen: ["agent-abc123", "agent-def456"],
      });
      expect(
        attentionSatisfies(next!, {
          kind: "event",
          id: "EVT-1",
          sessionId: "SES-1",
          agentOpen: ["agent-abc123", "agent-def456"],
        }),
      ).toBe(true);
    });

    it("should normalize agentOpen ids (trim, unique, stable sort)", () => {
      const next = foldIntent(base, {
        kind: "event",
        id: "EVT-1",
        sessionId: "SES-1",
        agentOpen: ["  agent-z ", "agent-a", "agent-z"],
      });
      expect(next?.route.storyAgentOpen).toEqual(["agent-a", "agent-z"]);
    });
  });

  it("session + gantt → canvas mode + selection", () => {
    const next = foldIntent(base, {
      kind: "session",
      id: "SES-1",
      canvasMode: "gantt",
    });
    expect(next?.route.view).toBe("canvas");
    expect(next?.canvas).toMatchObject({
      mode: "gantt",
      selectedSessionId: "SES-1",
    });
    expect(
      attentionSatisfies(next!, {
        kind: "session",
        id: "SES-1",
        canvasMode: "gantt",
      }),
    ).toBe(true);
  });

  it("canvas groupBy + metric lift into Attention", () => {
    const next = foldIntent(base, {
      kind: "canvas",
      id: "canvas",
      canvasMode: "sunburst",
      groupBy: "agent",
      metric: "tokens",
    });
    expect(next?.canvas).toMatchObject({
      mode: "sunburst",
      groupBy: "agent",
      metric: "tokens",
    });
  });

  it("canvas expandKeys open board group/project drill path", () => {
    const next = foldIntent(base, {
      kind: "canvas",
      id: "canvas",
      canvasMode: "board",
      groupBy: "user",
      expandKeys: ["g:maxglassie", "p:maxglassie:OpenStory"],
    });
    expect(next?.route.view).toBe("canvas");
    expect(next?.canvas).toMatchObject({
      mode: "board",
      groupBy: "user",
      expandedKeys: ["g:maxglassie", "p:maxglassie:OpenStory"],
    });
    expect(
      attentionSatisfies(next!, {
        kind: "canvas",
        id: "canvas",
        canvasMode: "board",
        expandKeys: ["g:maxglassie", "p:maxglassie:OpenStory"],
      }),
    ).toBe(true);
  });

  // Dual-inject collapse: sequence path (plan → foldSteps / realizeIntent)
  // always commits Attention.expandedKeys first; materialize must not re-inject
  // canvas.expand (SessionsCanvas paints from canvasAttention$).
  describe("when sequence path commits expandKeys on Attention first", () => {
    const keys = ["g:maxglassie", "p:maxglassie:OpenStory"] as const;

    it("should land expandKeys via foldSteps before materialize, and not dual-inject canvas.expand", () => {
      const steps = planNavigateTo({
        kind: "canvas",
        id: "canvas",
        canvasMode: "board",
        expandKeys: [...keys],
      });
      expect(steps).not.toBeNull();
      expect(
        steps!.some(
          (s) =>
            s.action === "set" &&
            (s.params as { target?: string }).target === "canvas.expand",
        ),
      ).toBe(true);

      // Sequence denotation: foldSteps commits Attention (source of truth).
      const att = foldSteps(emptyAttention(), steps!, interpretControl);
      expect(att.canvas.expandedKeys).toEqual([...keys]);
      expect(
        attentionSatisfies(att, {
          kind: "canvas",
          id: "canvas",
          canvasMode: "board",
          expandKeys: [...keys],
        }),
      ).toBe(true);

      // materialize must not re-inject canvas.expand — Attention already holds it.
      const injected: Array<{ action: string; target?: string }> = [];
      materializeAttention(att, {
        navigate: () => {},
        setSpotlight: () => {},
        setTitleCard: () => {},
        injectControl: (action, params) => {
          injected.push({
            action,
            target:
              typeof params.target === "string" ? params.target : undefined,
          });
        },
      });
      expect(
        injected.filter((i) => i.target === "canvas.expand"),
      ).toEqual([]);
    });
  });

  describe("when navigate_to lands scatter brush on Attention", () => {
    it("should fold scatterBrush into canvas + satisfy land predicate", () => {
      const brush = {
        ev0: 10,
        ev1: 100,
        tok0: 1000,
        tok1: 50_000,
        includeZero: false,
      };
      const next = foldIntent(base, {
        kind: "canvas",
        id: "canvas",
        canvasMode: "scatter",
        scatterBrush: brush,
      });
      expect(next?.route.view).toBe("canvas");
      expect(next?.canvas).toMatchObject({
        mode: "scatter",
        scatterBrush: brush,
      });
      expect(
        attentionSatisfies(next!, {
          kind: "canvas",
          id: "canvas",
          canvasMode: "scatter",
          scatterBrush: brush,
        }),
      ).toBe(true);
    });

    it("should foldControl set scatter.brush into Attention (data-space box)", () => {
      let a = emptyAttention({ view: "canvas" });
      a = foldControl(a, {
        type: "set",
        target: "scatter.brush",
        params: { ev0: 5, ev1: 50, tok0: 100, tok1: 10_000, includeZero: true },
      });
      expect(a.route.view).toBe("canvas");
      expect(a.canvas.scatterBrush).toEqual({
        ev0: 5,
        ev1: 50,
        tok0: 100,
        tok1: 10_000,
        includeZero: true,
      });
      expect(
        attentionSatisfies(a, {
          kind: "canvas",
          id: "canvas",
          scatterBrush: {
            ev0: 5,
            ev1: 50,
            tok0: 100,
            tok1: 10_000,
            includeZero: true,
          },
        }),
      ).toBe(true);
    });

    // Contract: ScatterView must paint from Attention (canvasAttention$), not
    // require control$ dual inject. Pure bridge: fold → canvas.scatterBrush → paint.
    it("should derive selecting + brushed ids from canvas.scatterBrush without control$", () => {
      const pt = (
        id: string,
        events: number,
        tokens: number,
      ): ScatterPoint => ({
        id,
        label: id,
        events,
        tokens,
        durationMs: 0,
        agent: "claude-code",
        zero: tokens <= 0,
      });
      const points = [
        pt("in", 50, 5000),
        pt("out-ev", 2, 5000),
        pt("out-tok", 50, 999_999),
      ];
      const brush = {
        ev0: 10,
        ev1: 100,
        tok0: 1000,
        tok1: 50_000,
        includeZero: false,
      };
      const next = foldIntent(base, {
        kind: "canvas",
        id: "canvas",
        canvasMode: "scatter",
        scatterBrush: brush,
      });
      expect(next).not.toBeNull();
      // Attention alone is enough — no injectControl / control$ in this path.
      const paint = scatterPaintFromBrush(points, next!.canvas.scatterBrush);
      expect(paint.selecting).toBe(true);
      expect(paint.brushed?.map((p) => p.id)).toEqual(["in"]);
      // Clear brush → idle paint (human deselect / no agent brush).
      const idle = scatterPaintFromBrush(points, undefined);
      expect(idle).toEqual({ selecting: false, brushed: null });
    });

    // Dual-inject collapse: sequence path (plan → foldSteps / realizeIntent)
    // always commits Attention.scatterBrush first; materialize must not re-inject
    // scatter.brush (ScatterView paints from canvasAttention$).
    describe("when sequence path commits scatterBrush on Attention first", () => {
      const brush = {
        ev0: 10,
        ev1: 100,
        tok0: 1000,
        tok1: 50_000,
        includeZero: false,
      };

      it("should land brush via foldSteps before materialize, and not dual-inject scatter.brush", () => {
        const steps = planNavigateTo({
          kind: "canvas",
          id: "canvas",
          canvasMode: "scatter",
          scatterBrush: brush,
        });
        expect(steps).not.toBeNull();
        expect(
          steps!.some(
            (s) =>
              s.action === "set" &&
              (s.params as { target?: string }).target === "scatter.brush",
          ),
        ).toBe(true);

        // Sequence denotation: foldSteps commits Attention (source of truth).
        const att = foldSteps(emptyAttention(), steps!, interpretControl);
        expect(att.canvas.scatterBrush).toEqual(brush);
        expect(
          attentionSatisfies(att, {
            kind: "canvas",
            id: "canvas",
            canvasMode: "scatter",
            scatterBrush: brush,
          }),
        ).toBe(true);

        // materialize must not re-inject scatter.brush — Attention already holds it.
        const injected: Array<{ action: string; target?: string }> = [];
        materializeAttention(att, {
          navigate: () => {},
          setSpotlight: () => {},
          setTitleCard: () => {},
          injectControl: (action, params) => {
            injected.push({
              action,
              target:
                typeof params.target === "string" ? params.target : undefined,
            });
          },
        });
        expect(
          injected.filter((i) => i.target === "scatter.brush"),
        ).toEqual([]);
      });
    });
  });

  it("foldControl set canvas.expand merges keys; toggle adds/removes", () => {
    let a = emptyAttention({ view: "canvas" });
    a = foldControl(a, {
      type: "set",
      target: "canvas.expand",
      params: { keys: ["g:user-a", "p:user-a:proj"] },
    });
    expect(a.canvas.expandedKeys).toEqual(["g:user-a", "p:user-a:proj"]);
    a = foldControl(a, {
      type: "toggle",
      target: "canvas.expand",
      value: "g:user-b",
    });
    expect(a.canvas.expandedKeys).toContain("g:user-b");
    a = foldControl(a, {
      type: "toggle",
      target: "canvas.expand",
      value: "g:user-a",
    });
    expect(a.canvas.expandedKeys).not.toContain("g:user-a");
    a = foldControl(a, {
      type: "set",
      target: "canvas.expand",
      params: { keys: [] },
    });
    expect(a.canvas.expandedKeys ?? []).toEqual([]);
  });

  it("foldControl toggles canvas.groupBy / metric", () => {
    let a = emptyAttention({ view: "canvas" });
    a = foldControl(a, { type: "toggle", target: "canvas.groupBy", value: "day" });
    a = foldControl(a, { type: "toggle", target: "canvas.metric", value: "tokens" });
    expect(a.canvas.groupBy).toBe("day");
    expect(a.canvas.metric).toBe("tokens");
  });

  it("property: foldIntent then attentionSatisfies for core intents", () => {
    const intents = [
      { kind: "event" as const, id: "E1", sessionId: "S1", details: true },
      { kind: "session" as const, id: "S1", canvasMode: "gantt" },
      { kind: "day" as const, id: "2026-07-27" },
      { kind: "file" as const, id: "foo.rs" },
      { kind: "person" as const, id: "max" },
      { kind: "project" as const, id: "OpenStory" },
      { kind: "reel" as const, id: "reel-abc123" },
    ];
    for (const intent of intents) {
      const next = foldIntent(base, intent);
      expect(next, JSON.stringify(intent)).not.toBeNull();
      expect(attentionSatisfies(next!, intent), JSON.stringify(intent)).toBe(true);
    }
  });

  it("day → explore filter", () => {
    const next = foldIntent(base, { kind: "day", id: "2026-07-27" });
    expect(next?.route.explore?.filters?.day).toBe("2026-07-27");
  });

  // Sidebar facet chips (facet-{group}-{key} testids) as named navigate_to entities.
  // Operational path already exists via query; this is chip-id / kind:facet land.
  describe("when navigate_to names a sidebar facet chip as entity", () => {
    it("should fold chip-id into explore filter and satisfy land", () => {
      const chipId = "facet-host-Katies-Mac-mini";
      const next = foldIntent(base, { kind: "facet", id: chipId });
      expect(next).not.toBeNull();
      expect(next?.route.view).toBe("explore");
      expect(next?.route.explore?.filters).toEqual({ host: "Katies-Mac-mini" });
      expect(
        attentionSatisfies(next!, { kind: "facet", id: chipId }),
      ).toBe(true);
    });

    it("should fold structured facet+value for every sidebar group", () => {
      const cases: Array<{ facet: string; id: string; filter: Record<string, string> }> = [
        { facet: "project", id: "OpenStory", filter: { project: "OpenStory" } },
        { facet: "status", id: "ongoing", filter: { status: "ongoing" } },
        { facet: "agent", id: "claude-code", filter: { agent: "claude-code" } },
        { facet: "user", id: "katie", filter: { user: "katie" } },
        { facet: "host", id: "mbp", filter: { host: "mbp" } },
        { facet: "branch", id: "feat/x", filter: { branch: "feat/x" } },
      ];
      for (const c of cases) {
        const intent = { kind: "facet" as const, facet: c.facet, id: c.id };
        const next = foldIntent(base, intent);
        expect(next, JSON.stringify(c)).not.toBeNull();
        expect(next?.route.explore?.filters, JSON.stringify(c)).toEqual(c.filter);
        expect(attentionSatisfies(next!, intent), JSON.stringify(c)).toBe(true);
      }
    });

    it("should plan query land pattern from facet chip-id", () => {
      const steps = planNavigateTo({
        kind: "facet",
        id: "facet-status-errored",
      });
      expect(steps).not.toBeNull();
      expect(steps![0]).toMatchObject({
        action: "query",
        params: { status: "errored" },
      });
      expect(steps![0]!.landPattern?.test("#/explore?status=errored")).toBe(true);
    });
  });

  it("file → search", () => {
    const next = foldIntent(base, { kind: "file", id: "a/b/c.rs" });
    expect(next?.route).toMatchObject({
      view: "explore",
      detailView: "search",
      searchQuery: "c.rs",
    });
  });

  it("reel → reels route with reelId", () => {
    const next = foldIntent(base, { kind: "reel", id: "reel-abc123" });
    expect(next?.route).toEqual({ view: "reels", reelId: "reel-abc123" });
    expect(attentionSatisfies(next!, { kind: "reel", id: "reel-abc123" })).toBe(true);
  });

  it("reel + autoplay → reelAutoplay set on the route", () => {
    const next = foldIntent(base, { kind: "reel", id: "reel-abc123", autoplay: true });
    expect(next?.route).toEqual({
      view: "reels",
      reelId: "reel-abc123",
      reelAutoplay: true,
    });
  });

  it("spotlight event keeps separate from route", () => {
    const next = foldIntent(base, {
      kind: "event",
      id: "EVT-1",
      sessionId: "SES-1",
      spotlight: true,
    });
    expect(next?.spotlight).toEqual({
      kind: "event",
      sessionId: "SES-1",
      eventId: "EVT-1",
    });
  });
});

describe("foldControl", () => {
  it("navigate clears spotlight", () => {
    const withSpot = foldIntent(emptyAttention(), {
      kind: "event",
      id: "E",
      sessionId: "S",
      spotlight: true,
    })!;
    const next = foldControl(withSpot, {
      type: "navigate",
      route: { view: "live" },
    });
    expect(next.spotlight).toBeNull();
    expect(next.route.view).toBe("live");
  });

  it("set story.details opens details on story route", () => {
    const base = emptyAttention({
      view: "story",
      sessionId: "S",
      eventId: "E",
    });
    const next = foldControl(base, {
      type: "set",
      target: "story.details",
      params: { open: true, sessionId: "S", eventId: "E" },
    });
    expect(next.route.storyDetails).toBe(true);
  });
});

describe("realizeIntent", () => {
  it("prefers direct foldIntent", () => {
    const next = realizeIntent(
      emptyAttention(),
      { kind: "person", id: "max" },
      interpretControl,
    );
    expect(next?.route.explore?.filters?.user).toBe("max");
  });
});

describe("attentionSummary", () => {
  it("is a stable one-liner", () => {
    const a = foldIntent(emptyAttention(), {
      kind: "event",
      id: "EVT-abcdef01",
      sessionId: "SES-12345678",
      details: true,
    })!;
    expect(attentionSummary(a)).toContain("story");
    expect(attentionSummary(a)).toContain("details");
  });
});
