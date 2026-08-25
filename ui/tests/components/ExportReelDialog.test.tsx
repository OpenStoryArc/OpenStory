/** ExportReelDialog — collect → scan → preview → save.
 *
 *  Fixtures here deliberately use `kind: "title"` stops rather than the
 *  default `spotlight` kind. Planted findings live in `BundleSlide.line`
 *  (always populated regardless of stage outcome — see `buildBundle` in
 *  reel-bundle.ts), so the scan-gate behavior under test doesn't depend on
 *  spotlight snapshot capture at all. This sidesteps mounting the real
 *  `EventSpotlight` (which does its own `createRoot` + async fetch) inside
 *  these specs; the collector's spotlight path is exercised by the running
 *  app, not by this component test. See task-5-report.md for the full seam
 *  writeup. */

import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { ExportReelDialog } from "@/components/reels/ExportReelDialog";
import { sanitizeSnapshotHtml } from "@/lib/export-sanitize";
import type { Reel } from "@/lib/reels-api";

// Passthrough by default — only the "spotlight capture throws" spec below
// overrides this (once) to prove a throw mid-capture degrades one slide
// instead of aborting the whole export. Every other spec in this file uses
// `kind: "title"` fixtures that never reach this function at all.
vi.mock("@/lib/export-sanitize", () => ({
  sanitizeSnapshotHtml: vi.fn((html: string) => html),
}));

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  window.localStorage?.clear();
});

function stubReelFetch(reel: Reel) {
  const fetchMock = vi.fn((url: string) => {
    const u = String(url);
    const m = u.match(/^\/api\/reels\/([^/?]+)$/);
    if (m && m[1] && decodeURIComponent(m[1]) === reel.id) {
      return Promise.resolve({ ok: true, json: () => Promise.resolve(reel) });
    }
    return Promise.resolve({ ok: false, status: 404, json: () => Promise.resolve(null) });
  });
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

const CLEAN_REEL: Reel = {
  id: "r1",
  title: "Clean Reel",
  created: "2026-08-01T10:00:00Z",
  author: "max",
  stops: [
    { line: "We fixed the caption bar.", kind: "title" },
    { line: "Then merged the PR.", kind: "title" },
  ],
};

const FINDING_REEL: Reel = {
  id: "r2",
  title: "Leaky Reel",
  created: "2026-08-01T10:00:00Z",
  author: "max",
  stops: [{ line: "key AKIAIOSFODNN7EXAMPLE was rotated", kind: "title" }],
};

const SPOTLIGHT_THROW_REEL: Reel = {
  id: "r3",
  title: "Partially Exportable",
  created: "2026-08-01T10:00:00Z",
  author: "max",
  stops: [
    // Default kind (no `kind` field) is "spotlight" — its capture is made
    // to throw below via the sanitizeSnapshotHtml mock.
    { sessionId: "s1", eventId: "e1", line: "This spotlight capture will throw." },
    { line: "This slide is fine.", kind: "title" },
  ],
};

describe("when the collected bundle scans clean", () => {
  it("should show 'Save reel file' as the primary action", async () => {
    stubReelFetch(CLEAN_REEL);
    render(<ExportReelDialog reelId="r1" onClose={vi.fn()} />);

    await waitFor(() =>
      expect(screen.getByTestId("export-reel-primary")).toBeInTheDocument(),
    );
    expect(screen.getByTestId("export-reel-primary")).toHaveTextContent("Save reel file");
  });
});

describe("when the collected bundle has a planted finding", () => {
  it("should show 'Export anyway' with the finding's family and slide id", async () => {
    stubReelFetch(FINDING_REEL);
    render(<ExportReelDialog reelId="r2" onClose={vi.fn()} />);

    await waitFor(() =>
      expect(screen.getByTestId("export-reel-primary")).toBeInTheDocument(),
    );
    expect(screen.getByTestId("export-reel-primary")).toHaveTextContent("Export anyway");
    expect(screen.getByText("aws-key")).toBeInTheDocument();
    expect(screen.getByText("r2:s0")).toBeInTheDocument();
  });

  it("should embed acknowledged:true in the downloaded file when exporting anyway", async () => {
    stubReelFetch(FINDING_REEL);
    // jsdom doesn't implement the Blob URL registry at all — define no-op
    // stand-ins first so `vi.spyOn` has a real method to wrap.
    if (typeof URL.createObjectURL !== "function") {
      (URL as unknown as { createObjectURL: (b: Blob) => string }).createObjectURL = () =>
        "blob:stub";
    }
    if (typeof URL.revokeObjectURL !== "function") {
      (URL as unknown as { revokeObjectURL: (u: string) => void }).revokeObjectURL = () => {};
    }
    const created: Blob[] = [];
    vi.spyOn(URL, "createObjectURL").mockImplementation((obj: Blob | MediaSource) => {
      created.push(obj as Blob);
      return "blob:mock-url";
    });
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});

    render(<ExportReelDialog reelId="r2" onClose={vi.fn()} />);
    await waitFor(() =>
      expect(screen.getByTestId("export-reel-primary")).toHaveTextContent("Export anyway"),
    );

    fireEvent.click(screen.getByTestId("export-reel-primary"));

    await waitFor(() => expect(created.length).toBe(1));
    // jsdom's Blob has no `.text()` — read it back via FileReader instead.
    const text = await new Promise<string>((resolve, reject) => {
      const reader = new FileReader();
      reader.onerror = () => reject(reader.error);
      reader.onload = () => resolve(String(reader.result));
      reader.readAsText(created[0]!);
    });
    expect(text).toContain('"acknowledged":true');
  });
});

describe("the export preview", () => {
  it("should embed the bundle JSON marker in the iframe srcdoc", async () => {
    stubReelFetch(CLEAN_REEL);
    render(<ExportReelDialog reelId="r1" onClose={vi.fn()} />);

    await waitFor(() => expect(screen.getByTestId("export-reel-iframe")).toBeInTheDocument());
    const iframe = screen.getByTestId("export-reel-iframe") as HTMLIFrameElement;
    expect(iframe.srcdoc).toContain('id="reel-bundle"');
  });

  it("should report the real finding count, never 'scan: clean', once findings exist", async () => {
    stubReelFetch(FINDING_REEL);
    render(<ExportReelDialog reelId="r2" onClose={vi.fn()} />);

    await waitFor(() =>
      expect(screen.getByTestId("export-reel-primary")).toHaveTextContent("Export anyway"),
    );
    // By the time the primary button reflects findings > 0, the previewed
    // iframe must already agree — buildBundle's placeholder
    // {findings:0, acknowledged:false} must have been folded over with the
    // real count before baking, not left in place.
    const iframe = screen.getByTestId("export-reel-iframe") as HTMLIFrameElement;
    expect(iframe.srcdoc).toContain('"findings":1');
    expect(iframe.srcdoc).not.toContain("scan: clean");
  });
});

describe("when a spotlight slide's capture throws", () => {
  it("should degrade that one slide and still export the rest of the reel", async () => {
    stubReelFetch(SPOTLIGHT_THROW_REEL);
    vi.mocked(sanitizeSnapshotHtml).mockImplementationOnce(() => {
      throw new Error("simulated sanitize crash mid-capture");
    });

    render(<ExportReelDialog reelId="r3" onClose={vi.fn()} />);

    // The export still completes — a throw during one slide's capture must
    // not reject collectBundle and hard-error the whole dialog.
    await waitFor(() =>
      expect(screen.getByTestId("export-reel-primary")).toHaveTextContent("Save reel file"),
    );

    // The slide whose capture threw is reported as degraded...
    expect(screen.getByTestId("export-reel-degraded")).toHaveTextContent("r3:s0");

    // ...while the other slide still made it into the baked bundle.
    const iframe = screen.getByTestId("export-reel-iframe") as HTMLIFrameElement;
    expect(iframe.srcdoc).toContain('"id":"r3:s1"');
  });
});
