import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { parseHash, buildHash, type HashRoute } from "@/lib/hash-route";

describe("parseHash", () => {
  const table: [string, string, HashRoute][] = [
    ["empty string",         "",                                    { view: "live" }],
    ["hash only",            "#",                                   { view: "live" }],
    ["#/",                   "#/",                                  { view: "live" }],
    ["#/live",               "#/live",                              { view: "live" }],
    ["#/live/SESSION",       "#/live/SESSION",                      { view: "live", sessionId: "SESSION" }],
    ["#/explore",            "#/explore",                           { view: "explore" }],
    ["#/explore/SESSION",    "#/explore/SES-123",                   { view: "explore", sessionId: "SES-123" }],
    ["#/explore + events",   "#/explore/SES/events",                { view: "explore", sessionId: "SES", detailView: "events" }],
    ["#/explore + convo",    "#/explore/SES/conversation",          { view: "explore", sessionId: "SES", detailView: "conversation" }],
    ["#/explore + plans",    "#/explore/SES/plans",                 { view: "explore", sessionId: "SES", detailView: "plans" }],
    ["#/explore + search",   "#/explore/SES/search",                { view: "explore", sessionId: "SES", detailView: "search" }],
    ["event deep-link",      "#/explore/SES/event/EVT-1",           { view: "explore", sessionId: "SES", eventId: "EVT-1", detailView: "events" }],
    ["file deep-link",       "#/explore/SES/file/src%2Fauth.rs",    { view: "explore", sessionId: "SES", filePath: "src/auth.rs" }],
    ["search query",         "#/search?q=fix+auth",                 { view: "explore", detailView: "search", searchQuery: "fix auth" }],
    ["search query encoded", "#/search?q=fix%20auth%20bug",         { view: "explore", detailView: "search", searchQuery: "fix auth bug" }],
    ["unknown view",         "#/unknown",                           { view: "live" }],
    ["invalid detail view",  "#/explore/SES/bogus",                 { view: "explore", sessionId: "SES" }],
  ];

  it.each(table)("%s → %o", (_label, input, expected) => {
    scenario(
      () => input,
      (hash) => parseHash(hash),
      (result) => expect(result).toEqual(expected),
    );
  });
});

describe("buildHash", () => {
  const table: [string, HashRoute, string][] = [
    ["live default",           { view: "live" },                                               "#/live"],
    ["live + session",         { view: "live", sessionId: "SES" },                             "#/live/SES"],
    ["explore default",        { view: "explore" },                                            "#/explore"],
    ["explore + session",      { view: "explore", sessionId: "SES" },                          "#/explore/SES"],
    ["explore + detail view",  { view: "explore", sessionId: "SES", detailView: "conversation" }, "#/explore/SES/conversation"],
    ["explore + event",        { view: "explore", sessionId: "SES", eventId: "EVT" },          "#/explore/SES/event/EVT"],
    ["explore + file",         { view: "explore", sessionId: "SES", filePath: "src/auth.rs" }, "#/explore/SES/file/src%2Fauth.rs"],
    ["search query",           { view: "explore", detailView: "search", searchQuery: "fix auth" }, "#/search?q=fix+auth"],
    ["search without query",   { view: "explore", detailView: "search" },                      "#/explore/search"],
  ];

  it.each(table)("%s → %s", (_label, input, expected) => {
    scenario(
      () => input,
      (route) => buildHash(route),
      (hash) => expect(hash).toBe(expected),
    );
  });
});

describe("parseHash ∘ buildHash roundtrip", () => {
  const routes: HashRoute[] = [
    { view: "live" },
    { view: "live", sessionId: "abc-123" },
    { view: "explore" },
    { view: "explore", sessionId: "abc-123" },
    { view: "explore", sessionId: "abc", detailView: "conversation" },
    // Event routes land on the Events detail view (where scroll-to-event
    // + auto-expand live) — the zoom fix of 2026-07-14.
    { view: "explore", sessionId: "abc", eventId: "evt-1", detailView: "events" },
    // The event→turn canopy edge: Story deep-links to the turn containing an
    // event. StoryView already consumes route.eventId; the parse must keep it.
    { view: "story", sessionId: "abc", eventId: "evt-1" },
    { view: "explore", sessionId: "abc", filePath: "src/main.rs" },
    { view: "explore", detailView: "search", searchQuery: "hello world" },
    { view: "live", userFilter: "katie" },
    { view: "live", sessionId: "abc-123", userFilter: "maxglassie" },
    { view: "story", userFilter: "katie" },
    { view: "live", timeFilter: "1h" },
    { view: "live", timeFilter: "today" },
    { view: "live", timeFilter: "week" },
    { view: "live", userFilter: "katie", timeFilter: "today" },
    { view: "live", sessionId: "sess-1", userFilter: "katie", timeFilter: "week" },
  ];

  it.each(routes)("roundtrip: %o", (route) => {
    scenario(
      () => route,
      (r) => parseHash(buildHash(r)),
      (result) => expect(result).toEqual(route),
    );
  });
});

describe("explore — bookmarkable filter state (query tail on any explore route)", () => {
  it("round-trips filters + sort on the bare explore route", () => {
    const route = { view: "explore", explore: { filters: { project: "OpenStory", range: "7d" }, sort: "events" } } as const;
    const hash = buildHash(route);
    expect(hash).toBe("#/explore?project=OpenStory&range=7d&sort=events");
    expect(parseHash(hash)).toEqual(route);
  });

  it("keeps filters when a session and detail view are in the path", () => {
    const route = { view: "explore", sessionId: "sess-9", detailView: "events", explore: { filters: { user: "max" } } } as const;
    const hash = buildHash(route);
    expect(hash).toBe("#/explore/sess-9/events?user=max");
    expect(parseHash(hash)).toEqual(route);
  });

  it("parses q as the sidebar search filter", () => {
    expect(parseHash("#/explore?q=fix+auth")).toEqual({ view: "explore", explore: { filters: { search: "fix auth" } } });
  });

  it("drops an invalid range and an invalid sort silently", () => {
    expect(parseHash("#/explore?range=90d&sort=bogus")).toEqual({ view: "explore" });
  });

  it("returns bare #/explore when there is no state", () => {
    expect(buildHash({ view: "explore" })).toBe("#/explore");
    expect(parseHash("#/explore")).toEqual({ view: "explore" });
  });
});

describe("story expand flags — details / eval / events / apply", () => {
  it("round-trips details + eval + events + apply indices", () => {
    const route: HashRoute = {
      view: "story",
      sessionId: "SES",
      eventId: "EVT",
      storyDetails: true,
      storyEvalOpen: true,
      storyEventsOpen: true,
      storyApplyOpen: [0, 2],
    };
    const hash = buildHash(route);
    expect(hash).toContain("details=1");
    expect(hash).toContain("eval=1");
    expect(hash).toContain("events=1");
    expect(hash).toContain("apply=0%2C2") || expect(hash).toContain("apply=0,2");
    expect(parseHash(hash)).toEqual(route);
  });

  it("parses apply=all as full apply expand", () => {
    expect(
      parseHash("#/story/SES/event/EVT?details=1&eval=1&apply=all"),
    ).toMatchObject({
      view: "story",
      sessionId: "SES",
      eventId: "EVT",
      storyDetails: true,
      storyEvalOpen: true,
      storyApplyOpen: "all",
    });
  });

  it("buildHash writes apply=all", () => {
    expect(
      buildHash({
        view: "story",
        sessionId: "S",
        eventId: "E",
        storyDetails: true,
        storyEvalOpen: true,
        storyApplyOpen: "all",
      }),
    ).toMatch(/apply=all/);
  });
});

describe("legacy #/overview links — parse-time alias onto Explore", () => {
  it("lands a bare #/overview on the Explore tab", () => {
    expect(parseHash("#/overview")).toEqual({ view: "explore" });
  });

  it("translates the full legacy query (facets, q, sort, sid→path sessionId)", () => {
    expect(parseHash("#/overview?project=OpenStory&sort=events&day=2026-06-30&q=fix+auth&sid=sess-9")).toEqual({
      view: "explore",
      sessionId: "sess-9",
      explore: { filters: { project: "OpenStory", day: "2026-06-30", search: "fix auth" }, sort: "events" },
    });
  });

  it("drops an unknown legacy sort silently", () => {
    expect(parseHash("#/overview?sort=bogus")).toEqual({ view: "explore" });
  });

  it("never builds #/overview again", () => {
    // The alias is parse-only; buildHash knows no overview view.
    expect(buildHash(parseHash("#/overview?status=ongoing"))).toBe("#/explore?status=ongoing");
  });
});

describe("legacy #/heatmap links — alias onto Canvas (heatmap is a mode there now)", () => {
  it("lands on the Canvas tab", () => {
    expect(parseHash("#/heatmap")).toEqual({ view: "canvas" });
    expect(buildHash(parseHash("#/heatmap"))).toBe("#/canvas");
  });
});

describe("retired lab + storm tabs", () => {
  it("aliases #/lab onto Canvas (its graduated shapes live there)", () => {
    expect(parseHash("#/lab")).toEqual({ view: "canvas" });
  });

  it("lets #/storm fall back to Live like any unknown route", () => {
    expect(parseHash("#/storm")).toEqual({ view: "live" });
    expect(parseHash("#/storm?sticky=read-model")).toEqual({ view: "live" });
  });
});

describe("userFilter — Live tab query param", () => {
  it("buildHash places ?user=… after the path", () => {
    expect(buildHash({ view: "live", userFilter: "katie" })).toBe(
      "#/live?user=katie",
    );
    expect(
      buildHash({ view: "live", sessionId: "sess-1", userFilter: "katie" }),
    ).toBe("#/live/sess-1?user=katie");
  });

  it("parseHash recovers userFilter from the query", () => {
    expect(parseHash("#/live?user=katie")).toEqual({
      view: "live",
      userFilter: "katie",
    });
    expect(parseHash("#/live/sess-1?user=maxglassie")).toEqual({
      view: "live",
      sessionId: "sess-1",
      userFilter: "maxglassie",
    });
  });

  it("ignores userFilter on tabs that don't apply (users, explore)", () => {
    // Users tab lists *all* users — no per-user filter.
    expect(buildHash({ view: "users", userFilter: "katie" } as HashRoute)).toBe(
      "#/users",
    );
    // Explore filters via its own searchQuery, not userFilter.
    expect(
      buildHash({ view: "explore", userFilter: "katie" } as HashRoute),
    ).toBe("#/explore");
  });

  it("URL-encodes special characters in the user value", () => {
    expect(
      buildHash({ view: "live", userFilter: "katie loughran" }),
    ).toMatch(/user=katie\+loughran/);
  });
});

describe("timeFilter — Live tab query param", () => {
  it("buildHash places ?time=… after the path", () => {
    expect(buildHash({ view: "live", timeFilter: "today" })).toBe(
      "#/live?time=today",
    );
    expect(buildHash({ view: "live", sessionId: "sess-1", timeFilter: "1h" })).toBe(
      "#/live/sess-1?time=1h",
    );
  });

  it("omits ?time= when the filter is 'all' (the implicit default)", () => {
    // Keeps URLs clean — bookmarking a "no time filter" state shouldn't
    // produce a noisy `?time=all` segment.
    expect(buildHash({ view: "live", timeFilter: "all" })).toBe("#/live");
  });

  it("composes ?user= and ?time= when both are set", () => {
    const built = buildHash({
      view: "live",
      userFilter: "katie",
      timeFilter: "week",
    });
    // URLSearchParams orders by insertion (`user` first, then `time`).
    expect(built).toBe("#/live?user=katie&time=week");
  });

  it("parseHash recovers timeFilter from the query", () => {
    expect(parseHash("#/live?time=today")).toEqual({
      view: "live",
      timeFilter: "today",
    });
    expect(parseHash("#/live?user=katie&time=1h")).toEqual({
      view: "live",
      userFilter: "katie",
      timeFilter: "1h",
    });
  });

  it("silently drops an unknown ?time= value rather than 400'ing the UI", () => {
    expect(parseHash("#/live?time=garbage")).toEqual({ view: "live" });
  });

  it("ignores timeFilter on tabs that don't apply (users, explore)", () => {
    expect(
      buildHash({ view: "users", timeFilter: "today" } as HashRoute),
    ).toBe("#/users");
    expect(
      buildHash({ view: "explore", timeFilter: "today" } as HashRoute),
    ).toBe("#/explore");
  });
});
