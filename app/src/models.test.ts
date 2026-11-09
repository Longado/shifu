import { describe, expect, it } from "vitest";
import { effortsFor, mergeCatalog, validPick, type ModelInfo } from "./models";

const m = (id: string, efforts: string[]): ModelInfo => ({ id, label: id, efforts });
const models = [m("opus", ["low", "high", "max"]), m("sonnet", ["low", "medium", "high"]), m("haiku-4-5", [])];

describe("models", () => {
  it("levels follow the chosen model; with the default every level appears once, in order", () => {
    expect(effortsFor(models, "haiku-4-5")).toEqual([]);
    expect(effortsFor(models, "sonnet")).toEqual(["low", "medium", "high"]);
    expect(effortsFor(models, "")).toEqual(["low", "high", "max", "medium"]);
  });

  it("a saved pick no longer on offer falls back to the default; with no list yet it is kept", () => {
    expect(validPick(models, { model: "opus", effort: "max" })).toEqual({ model: "opus", effort: "max" });
    expect(validPick(models, { model: "sonnet", effort: "max" })).toEqual({ model: "sonnet", effort: "" });
    expect(validPick(models, { model: "gone", effort: "low" })).toEqual({ model: "", effort: "low" });
    expect(validPick([], { model: "opus", effort: "max" })).toEqual({ model: "opus", effort: "max" });
  });

  it("a failed probe keeps last time's list", () => {
    const old = { claude: [m("opus", [])], codex: [m("gpt", [])] };
    const next = mergeCatalog(old, [
      { kind: "claude", models: [m("fable", [])], error: null },
      { kind: "codex", models: [], error: "读不到" },
      { kind: "codebuddy", models: [m("hy4free", [])], error: null },
    ]);
    expect(Object.fromEntries(Object.entries(next).map(([k, v]) => [k, v.map((x) => x.id)]))).toEqual({
      claude: ["fable"],
      codex: ["gpt"],
      codebuddy: ["hy4free"],
    });
  });
});
