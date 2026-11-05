import { describe, expect, it } from "vitest";
import { engineSupports, formatRate, splitRate, toBytesPerSecond } from "./speed";

describe("speed-limit values (CORE-06)", () => {
  it("converts decimal KB/s and MB/s to exact bytes per second", () => {
    expect(toBytesPerSecond("512", "kb")).toBe("512000");
    expect(toBytesPerSecond("1.024", "kb")).toBe("1024");
    expect(toBytesPerSecond("2,5", "mb")).toBe("2500000");
    expect(toBytesPerSecond(" 10 ", "mb")).toBe("10000000");
    expect(toBytesPerSecond("১০০", "kb")).toBe("100000"); // Bangla digits
    expect(toBytesPerSecond("1000000", "mb")).toBe("1000000000000");
  });
  it("refuses what the contract refuses", () => {
    for (const bad of ["", "0", "1", "1.023", "-5", "1e3", "1.2345", "abc", "1000001"]) {
      expect(toBytesPerSecond(bad, bad === "1000001" ? "mb" : "kb"), bad).toBeNull();
    }
  });
  it("splits stored values back for editing and formats them for reading", () => {
    expect(splitRate("2000000")).toEqual({ value: "2", unit: "mb" });
    expect(splitRate("512000")).toEqual({ value: "512", unit: "kb" });
    expect(splitRate("1024")).toEqual({ value: "1.024", unit: "kb" });
    expect(splitRate("2500000")).toEqual({ value: "2500", unit: "kb" });
    expect(formatRate("512000")).toBe("512 KB/s");
    expect(formatRate("2500000")).toBe("2.5 MB/s");
    expect(formatRate("1000000000000")).toBe("1000 GB/s");
    expect(formatRate("1024")).toBe("1 KB/s");
  });
  it("reads support only from the engine's capability report", () => {
    const backend = (support: "supported" | "unverified") => ({
      id: "http",
      version: "1",
      capabilities: [{ capability: "speed_limit_global" as const, support }],
    });
    expect(engineSupports(backend("supported"), "speed_limit_global")).toBe(true);
    expect(engineSupports(backend("unverified"), "speed_limit_global")).toBe(false);
    expect(engineSupports(backend("supported"), "speed_limit_per_job")).toBe(false);
    expect(engineSupports(undefined, "speed_limit_global")).toBe(false);
  });
});
