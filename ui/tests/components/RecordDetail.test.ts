//! Spec: RecordDetail helpers — formatBytes boundary table.

import { describe, it, expect } from "vitest";
import { formatBytes } from "@/lib/truncation";

describe("formatBytes", () => {
  // Boundary table:
  // | bytes    | expected    |
  // | 0        | "0 B"       |
  // | 100      | "100 B"     |
  // | 1023     | "1023 B"    |
  // | 1024     | "1.0 KB"    |
  // | 2048     | "2.0 KB"    |
  // | 45200    | "44.1 KB"   |
  // | 1048576  | "1.0 MB"    |
  // | 5242880  | "5.0 MB"    |

  const cases: [number, string][] = [
    [0, "0 B"],
    [100, "100 B"],
    [1023, "1023 B"],
    [1024, "1.0 KB"],
    [2048, "2.0 KB"],
    [45200, "44.1 KB"],
    [1048576, "1.0 MB"],
    [5242880, "5.0 MB"],
  ];

  it.each(cases)("%d bytes → %s", (bytes, expected) => {
    expect(formatBytes(bytes)).toBe(expected);
  });
});
