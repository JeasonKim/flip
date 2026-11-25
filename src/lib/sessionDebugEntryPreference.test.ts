import { describe, expect, it, vi } from "vitest";
import {
  recallDebugEntryVisibility,
  rememberDebugEntryVisibility,
} from "./sessionDebugEntryPreference";

function memoryStorage(initial: Record<string, string> = {}): Storage {
  const values = new Map(Object.entries(initial));
  return {
    get length() {
      return values.size;
    },
    clear: () => values.clear(),
    getItem: (key: string) => values.get(key) ?? null,
    key: (index: number) => [...values.keys()][index] ?? null,
    removeItem: (key: string) => void values.delete(key),
    setItem: (key: string, value: string) => void values.set(key, value),
  };
}

describe("session debug entry preference", () => {
  it("keeps debug entries hidden until the user turns them on", () => {
    expect(recallDebugEntryVisibility(memoryStorage())).toBe(false);
    expect(recallDebugEntryVisibility(null)).toBe(false);
  });

  it("restores the remembered choice across sessions", () => {
    const storage = memoryStorage();
    rememberDebugEntryVisibility(storage, true);
    expect(recallDebugEntryVisibility(storage)).toBe(true);

    rememberDebugEntryVisibility(storage, false);
    expect(recallDebugEntryVisibility(storage)).toBe(false);
  });

  it("warns and falls back to hidden when the stored preference is unusable", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    expect(
      recallDebugEntryVisibility(
        memoryStorage({ "flip.sessions.debugEntriesVisible": "yes" }),
      ),
    ).toBe(false);
    expect(warn).toHaveBeenCalledOnce();
    warn.mockRestore();
  });

  it("warns and stays usable when storage rejects reads or writes", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const blockedStorage = {
      ...memoryStorage(),
      getItem: () => {
        throw new Error("storage blocked");
      },
      setItem: () => {
        throw new Error("storage blocked");
      },
    } as Storage;

    expect(recallDebugEntryVisibility(blockedStorage)).toBe(false);
    expect(() => rememberDebugEntryVisibility(blockedStorage, true)).not.toThrow();
    expect(warn).toHaveBeenCalledTimes(2);
    warn.mockRestore();
  });
});
