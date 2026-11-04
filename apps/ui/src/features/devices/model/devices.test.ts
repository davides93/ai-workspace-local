import { describe, expect, it } from "vitest";
import type { Accelerator, DeviceProfile } from "../../../shared/api/types";
import { budgetRule, isUnifiedMemory, memoryUsage, modelBudgetMb, vramMb } from "./hardware";
import { digitsOf, groupCode, isPaired, normAddress, slug, validatePair } from "./mesh";

const GB = 1024;
const acc = (kind: Accelerator["kind"], memory_mb?: number): Accelerator => ({ kind, name: kind, memory_mb });
const dev = (os: DeviceProfile["os"], arch: string, ramGb: number, accelerators: Accelerator[]) => ({ os, arch, memory_mb: ramGb * GB, accelerators });

// The same cases as the Rust tests of `aiw_hw::model_budget_mb`.
describe("modelBudgetMb", () => {
  it("is the larger of the dedicated VRAM and half of the RAM", () => {
    expect(modelBudgetMb(dev("windows", "x86_64", 64, [acc("cuda", 24 * GB), acc("cpu")]))).toBe(32 * GB);
    expect(modelBudgetMb(dev("linux", "x86_64", 16, [acc("cuda", 24 * GB)]))).toBe(24 * GB);
    expect(modelBudgetMb(dev("linux", "x86_64", 16, [acc("cpu")]))).toBe(8 * GB);
  });
  it("gives unified-memory devices 70% of the RAM, never all of it", () => {
    const mac = dev("macos", "aarch64", 16, [acc("metal", 16 * GB)]);
    expect(isUnifiedMemory(mac)).toBe(true);
    expect(modelBudgetMb(mac)).toBe(11468);
    expect(modelBudgetMb(dev("macos", "aarch64", 16, [acc("metal")]))).toBe(11468);
  });
  it("honours a lower GPU limit on unified memory", () => {
    expect(modelBudgetMb(dev("macos", "aarch64", 16, [acc("metal", 8192)]))).toBe(8192);
  });
  it("treats phones as unified and an Intel Mac as not", () => {
    expect(modelBudgetMb(dev("android", "aarch64", 12, [acc("vulkan", 12 * GB)]))).toBe(8601);
    expect(isUnifiedMemory(dev("macos", "x86_64", 16, [acc("metal", 4 * GB)]))).toBe(false);
  });
});

describe("vramMb", () => {
  it("ignores the CPU and takes the largest accelerator", () => {
    expect(vramMb({ accelerators: [acc("cpu", 99999), acc("cuda", 8000), acc("vulkan", 12000), acc("npu")] })).toBe(12000);
    expect(vramMb({ accelerators: [] })).toBe(0);
  });
});

describe("budgetRule", () => {
  it("explains where the budget comes from", () => {
    expect(budgetRule(dev("macos", "aarch64", 16, [acc("metal")]))).toMatch(/70%/);
    expect(budgetRule(dev("linux", "x86_64", 16, [acc("cpu")]))).toMatch(/Nessuna GPU/);
    expect(budgetRule(dev("windows", "x86_64", 32, [acc("cuda", 24 * GB)]))).toMatch(/GPU è il limite/);
    expect(budgetRule(dev("windows", "x86_64", 64, [acc("cuda", 4 * GB)]))).toMatch(/Poca memoria/);
  });
});

describe("memoryUsage", () => {
  it("colours the meter by how full the RAM is", () => {
    expect(memoryUsage({ memory_mb: 1000, available_memory_mb: 600 })).toMatchObject({ used: 400, tone: "ok" });
    expect(memoryUsage({ memory_mb: 1000, available_memory_mb: 250 }).tone).toBe("warn");
    expect(memoryUsage({ memory_mb: 1000, available_memory_mb: 100 }).tone).toBe("crit");
    expect(memoryUsage({ memory_mb: 0, available_memory_mb: 0 })).toMatchObject({ used: 0, ratio: 0 });
    expect(memoryUsage({ memory_mb: 1000, available_memory_mb: 1500 }).used).toBe(0);
  });
});

describe("mesh helpers", () => {
  it("normalises addresses like the engine", () => {
    expect(normAddress("192.168.1.20")).toBe("http://192.168.1.20:7777");
    expect(normAddress(" http://192.168.1.20:7777/ ")).toBe("http://192.168.1.20:7777");
    expect(normAddress("host.local:8000")).toBe("http://host.local:8000");
    expect(normAddress("[::1]")).toBe("http://[::1]:7777");
  });
  it("builds peer ids like the engine", () => {
    expect(slug("MacBook Pro M4 Max")).toBe("macbook-pro-m4-max");
    expect(slug("  --Studio PC--  ")).toBe("studio-pc");
    expect(slug("x".repeat(40))).toHaveLength(32);
  });
  it("recognises a discovered device that is already paired, by address or by id", () => {
    const peers = [{ id: "macbook-pro", address: "192.168.1.42" }];
    expect(isPaired(peers, { id: "other", address: "http://192.168.1.42:7777" })).toBe(true);
    expect(isPaired(peers, { id: "MacBook Pro", address: "10.0.0.9" })).toBe(true);
    expect(isPaired(peers, { id: "nas", address: "192.168.1.31" })).toBe(false);
  });
  it("accepts a pairing code however it is grouped", () => {
    expect(digitsOf("482 913")).toBe("482913");
    expect(digitsOf("482-913")).toBe("482913");
    expect(groupCode("482913")).toBe("482 913");
    expect(groupCode("4829")).toBe("4829");
  });
  it("validates the pair-by-address form", () => {
    expect(validatePair({ address: "", code: "" })).toEqual({ address: expect.any(String), code: expect.any(String) });
    expect(validatePair({ address: "192.168.1.20", code: "12345" }).code).toBeDefined();
    expect(validatePair({ address: "192.168.1.20", code: "123 456" })).toEqual({});
  });
});
