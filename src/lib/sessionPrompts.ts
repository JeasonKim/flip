import type { SessionPrompt } from "../types/profile";

export type SessionPromptScope = "custom" | "all";

export function isCustomSessionPrompt(prompt: SessionPrompt): boolean {
  return prompt.origin === "configured" || prompt.origin === "request";
}

export function defaultPromptScope(prompts: readonly SessionPrompt[]): SessionPromptScope {
  return prompts.some(isCustomSessionPrompt) ? "custom" : "all";
}

function promptPriority(prompt: SessionPrompt): number {
  if (isCustomSessionPrompt(prompt)) return 0;
  return prompt.origin === "snapshot" ? 1 : 2;
}

export function prioritizeSessionPrompts(prompts: readonly SessionPrompt[]): SessionPrompt[] {
  return [...prompts].sort((left, right) => promptPriority(left) - promptPriority(right));
}

export function visibleSessionPrompts(
  prompts: readonly SessionPrompt[],
  scope: SessionPromptScope,
  query: string,
): SessionPrompt[] {
  const term = query.trim().toLowerCase();
  return prompts.filter(
    (prompt) =>
      (scope === "all" || isCustomSessionPrompt(prompt)) &&
      (!term || prompt.text.toLowerCase().includes(term)),
  );
}
