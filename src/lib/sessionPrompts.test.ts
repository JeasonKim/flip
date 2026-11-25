import { describe, expect, it } from "vitest";
import type { SessionPrompt } from "../types/profile";
import {
  defaultPromptScope,
  prioritizeSessionPrompts,
  visibleSessionPrompts,
} from "./sessionPrompts";

function prompt(origin: SessionPrompt["origin"], index: number, text: string): SessionPrompt {
  return { role: "system", origin, text, entrypoint: null, source: { section: "jsonl", index, record_id: null } };
}

describe("system prompt browsing", () => {
  const model = prompt("model", 1, "Model template");
  const unknown = prompt("unknown", 2, "Context for RELEASE CHECK");
  const snapshot = prompt("snapshot", 5, "Assembled system prompt for Release Check");
  const configured = prompt("configured", 8, "Own configured prompt");
  const request = prompt("request", 9, "Send user prompt");

  it("prefers confirmed custom prompts and preserves their relative order", () => {
    const records = [model, unknown, snapshot, configured, request];
    const ordered = prioritizeSessionPrompts(records);
    expect(defaultPromptScope(ordered)).toBe("custom");
    expect(visibleSessionPrompts(ordered, "custom", "")).toEqual([configured, request]);
    expect(visibleSessionPrompts(ordered, "custom", "release check")).toEqual([]);
    expect(visibleSessionPrompts(ordered, "all", "")).toEqual([configured, request, snapshot, model, unknown]);
  });

  it("opens the snapshot and searches bodies when the transcript has no custom attribution", () => {
    const ordered = prioritizeSessionPrompts([model, unknown, snapshot]);
    expect(defaultPromptScope(ordered)).toBe("all");
    expect(ordered[0]).toBe(snapshot);
    expect(visibleSessionPrompts(ordered, "all", "  release check  ")).toEqual([snapshot, unknown]);
    expect(visibleSessionPrompts(ordered, "all", "missing")).toEqual([]);
  });
});
