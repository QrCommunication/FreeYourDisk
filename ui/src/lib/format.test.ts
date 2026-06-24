import { describe, it, expect } from "vitest";
import { humanizeBytes, humanizeDate, usedPercent } from "./format";

describe("humanizeBytes", () => {
  it("formats across units", () => {
    expect(humanizeBytes(0)).toBe("0 B");
    expect(humanizeBytes(512)).toBe("512 B");
    expect(humanizeBytes(1536)).toBe("1.5 KB");
    expect(humanizeBytes(1024 * 1024)).toBe("1.0 MB");
    expect(humanizeBytes(5 * 1024 ** 3)).toBe("5.0 GB");
  });

  it("drops decimals for large values within a unit", () => {
    expect(humanizeBytes(900 * 1024)).toBe("900 KB");
  });
});

describe("usedPercent", () => {
  it("clamps and guards zero total", () => {
    expect(usedPercent(50, 100)).toBe(50);
    expect(usedPercent(0, 0)).toBe(0);
    expect(usedPercent(200, 100)).toBe(100);
  });
});

describe("humanizeDate", () => {
  it("returns null for null input", () => {
    expect(humanizeDate(null)).toBeNull();
  });
  it("formats a timestamp", () => {
    expect(humanizeDate(0)).not.toBeNull();
  });
});
