/** The agents the new-agent popover offers, the models each CLI reports on this machine (models.rs probes them;
 *  the last answer is kept in localStorage so the popover has a list before this launch's probe returns), and the
 *  last model/effort chosen per agent. "" means the CLI's own default (nothing is passed). */
import { modelsList } from "./api";
import type { Mark } from "./vendoricons";

export type AgentKind = Mark;

export interface AgentSpec {
  kind: AgentKind;
  name: string;
}

export const AGENTS: AgentSpec[] = [
  { kind: "claude", name: "Claude" },
  { kind: "codex", name: "Codex" },
  { kind: "codebuddy", name: "CodeBuddy" },
  { kind: "kimi", name: "Kimi" },
  { kind: "antigravity", name: "Antigravity" },
];

export const specOf = (kind: string): AgentSpec | undefined => AGENTS.find((a) => a.kind === kind);

/** Mirrors `ModelInfo` / `AgentModels` in app/src-tauri/src/models.rs. */
export interface ModelInfo {
  id: string;
  label: string;
  efforts: string[];
}
export interface AgentModels {
  kind: string;
  models: ModelInfo[];
  error: string | null;
}

const MODELS_KEY = "shifu.models";

function loadModels(): Record<string, ModelInfo[]> {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(MODELS_KEY) ?? "{}");
    return typeof v === "object" && v !== null ? (v as Record<string, ModelInfo[]>) : {};
  } catch {
    return {};
  }
}

let catalog = loadModels();

export const modelsOf = (kind: string): ModelInfo[] => catalog[kind] ?? [];

/** A probe that failed keeps the list from last time rather than emptying the picker. */
export function mergeCatalog(old: Record<string, ModelInfo[]>, fresh: readonly AgentModels[]): Record<string, ModelInfo[]> {
  const next = { ...old };
  for (const a of fresh) if (!a.error && a.models.length > 0) next[a.kind] = a.models;
  return next;
}

export async function refreshModels(): Promise<void> {
  catalog = mergeCatalog(catalog, await modelsList());
  localStorage.setItem(MODELS_KEY, JSON.stringify(catalog));
}

const EFFORT_LABELS: Record<string, string> = {
  minimal: "最低",
  low: "低",
  medium: "中",
  high: "高",
  xhigh: "很高",
  max: "最高",
  ultra: "极限",
};
export const effortLabel = (id: string): string => EFFORT_LABELS[id] ?? id;

/** Levels on offer: the chosen model's, or with the default model every level any model takes, in order. */
export function effortsFor(models: readonly ModelInfo[], model: string): string[] {
  const chosen = models.find((m) => m.id === model);
  if (chosen) return chosen.efforts;
  const all: string[] = [];
  for (const m of models) for (const e of m.efforts) if (!all.includes(e)) all.push(e);
  return all;
}

export interface Pick {
  model: string;
  effort: string;
}

const PICKS_KEY = "shifu.picks";

function loadPicks(): Record<string, Pick> {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(PICKS_KEY) ?? "{}");
    return typeof v === "object" && v !== null ? (v as Record<string, Pick>) : {};
  } catch {
    return {};
  }
}

/** The pick still has to be on offer; with no list yet (first launch) it is kept as saved. */
export function validPick(models: readonly ModelInfo[], p: Pick | undefined): Pick {
  if (!p) return { model: "", effort: "" };
  if (models.length === 0) return p;
  const model = models.some((m) => m.id === p.model) ? p.model : "";
  const effort = effortsFor(models, model).includes(p.effort) ? p.effort : "";
  return { model, effort };
}

export const pickFor = (kind: string): Pick => validPick(modelsOf(kind), loadPicks()[kind]);

export function savePick(kind: string, pick: Pick): void {
  localStorage.setItem(PICKS_KEY, JSON.stringify({ ...loadPicks(), [kind]: validPick(modelsOf(kind), pick) }));
}

/** Arguments for the backend: empty means "the CLI's default", sent as null. */
export const pickArgs = (p: Pick) => ({ model: p.model || null, effort: p.effort || null });
