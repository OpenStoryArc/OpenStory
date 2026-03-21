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
    ["event deep-link",      "#/explore/SES/event/EVT-1",           { view: "explore", sessionId: "SES", eventId: "EVT-1" }],
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
    { view: "explore", sessionId: "abc", eventId: "evt-1" },
    { view: "explore", sessionId: "abc", filePath: "src/main.rs" },
    { view: "explore", detailView: "search", searchQuery: "hello world" },
  ];

  it.each(routes)("roundtrip: %o", (route) => {
    scenario(
      () => route,
      (r) => parseHash(buildHash(r)),
      (result) => expect(result).toEqual(route),
    );
  });
});
