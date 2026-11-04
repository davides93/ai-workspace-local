import { describe, expect, it } from "vitest";
import { fileCountLabel, findJob, groupVariants, sharedRuntimes, sortVariants, splitRuntimes, summarizeFiles, visibleVariants } from "./variants";
import { job, rt, variant } from "./testdata";

const GB = 1024 ** 3;

describe("sortVariants", () => {
  it("puts the recommended variant first, then the smallest download to the largest", () => {
    const list = [
      variant({ id: "F16", size_bytes: 8 * GB }),
      variant({ id: "Q8_0", size_bytes: 4 * GB }),
      variant({ id: "Q4_K_M", size_bytes: 2 * GB, recommended: false }),
      variant({ id: "Q5_K_M", size_bytes: 3 * GB, recommended: true }),
    ];
    expect(sortVariants(list).map((v) => v.id)).toEqual(["Q5_K_M", "Q4_K_M", "Q8_0", "F16"]);
  });

  it("does not mutate the input and keeps the provider order on ties", () => {
    const list = [variant({ id: "b", size_bytes: 1 }), variant({ id: "a", size_bytes: 1 })];
    expect(sortVariants(list).map((v) => v.id)).toEqual(["b", "a"]);
    expect(list.map((v) => v.id)).toEqual(["b", "a"]);
  });
});

describe("groupVariants", () => {
  const list = [
    variant({ id: "st", format: "safetensors", size_bytes: 9 }),
    variant({ id: "q8", format: "gguf", size_bytes: 8 }),
    variant({ id: "q4", format: "gguf", size_bytes: 4, recommended: true }),
  ];

  it("groups by format with the group of the recommended variant first", () => {
    const groups = groupVariants(list);
    expect(groups.map((g) => g.format)).toEqual(["gguf", "safetensors"]);
    expect(groups[0].variants.map((v) => v.id)).toEqual(["q4", "q8"]);
  });

  it("keeps the provider order of the groups when none is recommended", () => {
    const groups = groupVariants(list.map((v) => ({ ...v, recommended: false })));
    expect(groups.map((g) => g.format)).toEqual(["safetensors", "gguf"]);
  });

  it("is empty for no variants", () => {
    expect(groupVariants([])).toEqual([]);
  });
});

describe("visibleVariants", () => {
  const list = [
    variant({ id: "ok", fit: "ok" }),
    variant({ id: "no", fit: "no" }),
    variant({ id: "no-installed", fit: "no", installed: true }),
    variant({ id: "no-rec", fit: "no", recommended: true }),
  ];
  it("hides the ones that do not fit, except the installed and the recommended", () => {
    expect(visibleVariants(list, true).map((v) => v.id)).toEqual(["ok", "no-installed", "no-rec"]);
    expect(visibleVariants(list, false)).toHaveLength(4);
  });
});

describe("files and runtimes", () => {
  it("counts the files and flags the vision projector", () => {
    const files = [
      { path: "m-00001-of-00002.gguf", size_bytes: 1, role: "weights" as const },
      { path: "m-00002-of-00002.gguf", size_bytes: 1, role: "weights" as const },
      { path: "mmproj-F16.gguf", size_bytes: 1, role: "mmproj" as const },
    ];
    expect(summarizeFiles(files)).toEqual({ count: 3, weights: 2, hasMmproj: true });
    expect(fileCountLabel(files)).toBe("3 file, con mmproj");
    expect(fileCountLabel(files.slice(0, 1))).toBe("1 file");
    expect(fileCountLabel([])).toBe("nessun file");
  });

  it("splits runtimes into ready and not ready, keeping the reason", () => {
    const { available, unavailable } = splitRuntimes([rt("a"), rt("b", false, "solo macOS"), rt("c")]);
    expect(available.map((r) => r.id)).toEqual(["a", "c"]);
    expect(unavailable).toEqual([rt("b", false, "solo macOS")]);
  });

  it("says the runtimes once only when every variant has exactly the same ones", () => {
    const same = [variant({ id: "a", runtimes: [rt("x"), rt("y", false, "da installare")] }), variant({ id: "b", runtimes: [rt("y", false, "da installare"), rt("x")] })];
    expect(sharedRuntimes(same)?.map((r) => r.id)).toEqual(["x", "y"]);
    expect(sharedRuntimes([variant({ id: "a", runtimes: [rt("x")] }), variant({ id: "b", runtimes: [rt("x", false)] })])).toBeNull();
    expect(sharedRuntimes([variant()])).toBeNull();
    expect(sharedRuntimes([])).toBeNull();
  });
});

describe("findJob", () => {
  const jobs = [
    job({ id: "1", model_ref: "a/b", variant: "Q4_K_M", status: "running" }),
    job({ id: "2", model_ref: "a/b", variant: "Q8_0", status: "done" }),
    job({ id: "3", model_ref: "a/c", variant: "", status: "queued" }),
    job({ id: "4", provider: "ollama", model_ref: "a/b", variant: "Q4_K_M", status: "running" }),
  ];
  it("ties an active job to (provider, model, variant)", () => {
    expect(findJob(jobs, { provider: "hf", model: "a/b", variant: "Q4_K_M" })?.id).toBe("1");
    expect(findJob(jobs, { provider: "ollama", model: "a/b", variant: "Q4_K_M" })?.id).toBe("4");
  });
  it("ignores finished jobs and other variants", () => {
    expect(findJob(jobs, { provider: "hf", model: "a/b", variant: "Q8_0" })).toBeUndefined();
    expect(findJob(jobs, { provider: "hf", model: "x/y", variant: "Q4_K_M" })).toBeUndefined();
  });
  it("an empty variant on the job (recommended chosen by the server) matches any variant of the model", () => {
    expect(findJob(jobs, { provider: "hf", model: "a/c", variant: "Q4_K_M" })?.id).toBe("3");
  });
});
