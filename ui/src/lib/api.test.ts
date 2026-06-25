import { describe, it, expect, vi, beforeEach } from "vitest";
import type { DeletionPlan } from "./api";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

const { api } = await import("./api");

describe("api", () => {
  beforeEach(() => invoke.mockReset());

  it("scan invokes with the service id", async () => {
    invoke.mockResolvedValue({
      result: { service: "temp", items: [], total_bytes: 0 },
      first_scan: true,
      new_ids: [],
    });
    const result = await api.scan("temp");
    expect(invoke).toHaveBeenCalledWith("scan", { service: "temp" });
    expect(result.result.total_bytes).toBe(0);
    expect(result.first_scan).toBe(true);
  });

  it("preview forwards the selection", async () => {
    invoke.mockResolvedValue({
      items: [],
      destination: "trash",
      total_bytes: 0,
      requires_root: false,
    });
    await api.preview("dev_cache", ["a", "b"]);
    expect(invoke).toHaveBeenCalledWith("preview", {
      service: "dev_cache",
      selection: ["a", "b"],
    });
  });

  it("execute passes the plan through", async () => {
    invoke.mockResolvedValue({ freed_bytes: 10, deleted_count: 1, errors: [] });
    const plan: DeletionPlan = {
      items: [],
      destination: "trash",
      total_bytes: 0,
      requires_root: false,
    };
    const report = await api.execute(plan);
    expect(invoke).toHaveBeenCalledWith("execute", { plan });
    expect(report.freed_bytes).toBe(10);
  });
});
