import { describe, expect, it } from "vitest";
import { formatDurationMs } from "./format-duration.ts";

describe("formatDurationMs", () => {
  it("formats sub-second as ms", () => {
    expect(formatDurationMs(0)).toBe("0ms");
    expect(formatDurationMs(842)).toBe("842ms");
  });

  it("formats seconds with sensible precision", () => {
    expect(formatDurationMs(7320)).toBe("7.32s");
    expect(formatDurationMs(4692)).toBe("4.69s");
    expect(formatDurationMs(32326)).toBe("32.3s");
    expect(formatDurationMs(5369)).toBe("5.37s");
  });

  it("formats minutes and hours", () => {
    expect(formatDurationMs(90_000)).toBe("1m 30s");
    expect(formatDurationMs(3_720_000)).toBe("1h 2m");
  });

  it("returns dash for invalid input", () => {
    expect(formatDurationMs(null)).toBe("—");
    expect(formatDurationMs(Number.NaN)).toBe("—");
  });
});
