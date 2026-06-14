import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { TopologyMap } from "@/components/admin/TopologyMap";
import type { Topology } from "@/lib/admin-api";

function topo(nodes: Topology["nodes"]): Topology {
  return {
    shape: "solo",
    self: {
      host: "a1",
      role: "solo",
      domain: null,
      hub_domain: null,
      peer_hub_domains: [],
      peer_domains: [],
    },
    nodes,
  };
}

describe("TopologyMap — solo with fleet evidence", () => {
  it("renders known hosts (self + hub + devices) in the shape, not just self", () => {
    const { container } = render(
      <TopologyMap
        topology={topo([
          { host: "a1", is_self: true, session_count: 261, source: "self-node" },
          { host: "100.77.40.95", is_self: false, session_count: 0, source: "nats-leafnode-hub" },
          { host: "Maxs-Air", is_self: false, session_count: 360, source: "sessions" },
        ])}
      />,
    );
    const text = container.textContent ?? "";
    expect(text).toContain("a1"); // self
    expect(text).toContain("Maxs-Air"); // the laptop — the whole point
    expect(text).toContain("100.77.40.95"); // the hub
  });

  it("falls back to a lone self card when there is no other evidence", () => {
    const { container } = render(
      <TopologyMap
        topology={topo([
          { host: "a1", is_self: true, session_count: 5, source: "self-node" },
        ])}
      />,
    );
    const text = container.textContent ?? "";
    expect(text).toContain("a1");
    expect(text).not.toContain("Maxs-Air");
  });
});
