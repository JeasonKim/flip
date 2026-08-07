import { describe, expect, it } from "vitest";
import {
  collectRawSearchMatches,
  resolveRawSearchCursor,
  type RawRecordSnapshot,
} from "./sessionRawSearch";

describe("session raw search", () => {
  it("collects case-insensitive matches from all raw records", () => {
    const records: RawRecordSnapshot[] = [
      { key: "jsonl-1", text: "Alpha beta alpha" },
      { key: "jsonl-2", text: "no match" },
      { key: "jsonl-3", text: "ALPHA" },
    ];

    const matches = collectRawSearchMatches(records, "alpha");

    expect(matches).toEqual([
      { recordKey: "jsonl-1", matchIndex: 0 },
      { recordKey: "jsonl-1", matchIndex: 11 },
      { recordKey: "jsonl-3", matchIndex: 0 },
    ]);
  });

  it("ignores blank keywords", () => {
    const records: RawRecordSnapshot[] = [{ key: "jsonl-1", text: "alpha" }];

    expect(collectRawSearchMatches(records, "   ")).toEqual([]);
  });

  it("moves the search cursor with wraparound", () => {
    expect(resolveRawSearchCursor(-1, 3, "next")).toBe(0);
    expect(resolveRawSearchCursor(-1, 3, "previous")).toBe(2);
    expect(resolveRawSearchCursor(2, 3, "next")).toBe(0);
    expect(resolveRawSearchCursor(0, 3, "previous")).toBe(2);
    expect(resolveRawSearchCursor(0, 0, "next")).toBe(-1);
  });
});
