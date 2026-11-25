import { describe, expect, it } from "vitest";
import type {
  SessionTimeline,
  SessionTimelineEntry,
  SessionTokenUsage,
} from "../types/profile";
import {
  flattenSessionTimeline,
  foldsIntoTimelineEntryGroup,
  formatTokenCountK,
  resolvePinnedTurnHeader,
  resolveTimelineEntryEmphasis,
  showsTimelineEntry,
  showsTimelineEntryPreview,
  summarizeGroupedEntries,
  visibleTokenUsageMetrics,
} from "./sessionTimeline";

const entries: SessionTimelineEntry[] = [
  {
    id: "meta-1",
    category: "metadata",
    importance: "debug",
    title: "Session",
    role: "system",
    content: "metadata",
    timestamp: 1,
    raw_reference: { section: "jsonl", index: 1, record_id: null },
  },
  {
    id: "user-1",
    category: "user",
    importance: "primary",
    title: "User",
    role: "user",
    content: "开始",
    timestamp: 2,
    raw_reference: { section: "jsonl", index: 2, record_id: null },
  },
  {
    id: "tool-1",
    category: "tool_call",
    importance: "detail",
    title: "Tool · Read",
    role: "tool",
    content: "读取文件",
    timestamp: 3,
    raw_reference: { section: "jsonl", index: 3, record_id: null },
  },
  {
    id: "event-1",
    category: "event",
    importance: "debug",
    title: "item_completed",
    role: "system",
    content: "内部完成事件",
    timestamp: 4,
    raw_reference: { section: "jsonl", index: 4, record_id: null },
  },
  {
    id: "assistant-1",
    category: "assistant",
    importance: "primary",
    title: "Assistant",
    role: "assistant",
    content: "完成",
    timestamp: 5,
    raw_reference: { section: "jsonl", index: 5, record_id: null },
  },
];

const usage: SessionTokenUsage = {
  total_tokens: 35_797,
  input_tokens: 35_758,
  output_tokens: 39,
  thinking_tokens: 12,
  cache_read_tokens: 34_560,
  cache_write_tokens: 0,
  cache_write_1h_tokens: 0,
  cache_write_5m_tokens: 0,
};

const timeline: SessionTimeline = {
  blocks: [
    {
      id: "context-meta-1",
      kind: "context",
      ordinal: null,
      status: null,
      usage: null,
      entry_start: 0,
      entry_end: 1,
    },
    {
      id: "turn-1-user-1",
      kind: "turn",
      ordinal: 1,
      status: "completed",
      usage,
      entry_start: 1,
      entry_end: 5,
    },
  ],
  entries,
};

describe("session timeline", () => {
  it("keeps small counts readable instead of rounding them down to zero", () => {
    expect(formatTokenCountK(35_758)).toBe("35.76K");
    expect(formatTokenCountK(1_000)).toBe("1K");
    expect(formatTokenCountK(999)).toBe("999");
    expect(formatTokenCountK(2)).toBe("2");
    expect(formatTokenCountK(0)).toBe("0");
  });

  it("keeps the requested metric order and omits only unavailable fields", () => {
    expect(
      visibleTokenUsageMetrics({ ...usage, total_tokens: null }).map(
        (metric) => `${metric.label} ${metric.value}`,
      ),
    ).toEqual([
      "Input 35.76K",
      "Output 39",
      "Thinking 12",
      "Read cache 34.56K",
      "Write cache 0",
    ]);
  });

  it("drops the breakdown columns that are always zero for this agent", () => {
    expect(
      visibleTokenUsageMetrics({
        ...usage,
        thinking_tokens: 0,
        cache_write_1h_tokens: 25_566,
        cache_write_5m_tokens: 0,
      }).map((metric) => metric.label),
    ).toEqual(["Total", "Input", "Output", "Read cache", "Write cache", "Write 1h"]);
  });

  it("keeps the shared columns visible even when an agent reports zero", () => {
    expect(
      visibleTokenUsageMetrics({
        ...usage,
        thinking_tokens: null,
        cache_write_1h_tokens: null,
        cache_write_5m_tokens: null,
      }).map((metric) => metric.label),
    ).toEqual(["Total", "Input", "Output", "Read cache", "Write cache"]);
  });

  it("preserves every event in its original timeline position", () => {
    const rows = flattenSessionTimeline(timeline, {
      collapsedBlockIds: new Set(),
      expandedEntryGroupIds: new Set(),
      debugEntriesVisible: true,
    });

    expect(rows.map((row) => row.id)).toEqual([
      "entry:meta-1",
      "group:turn-1-user-1",
      "entry:user-1",
      "entry-group:tool-1",
      "entry:assistant-1",
    ]);
  });

  it("folds consecutive side records into one collapsed group", () => {
    const rows = flattenSessionTimeline(timeline, {
      collapsedBlockIds: new Set(),
      expandedEntryGroupIds: new Set(),
      debugEntriesVisible: true,
    });
    const group = rows.find((row) => row.kind === "entry-group");

    expect(group?.entries.map((entry) => entry.id)).toEqual([
      "tool-1",
      "event-1",
    ]);
    expect(group?.summary).toBe("工具调用 1 · 事件 1");
    expect(group?.expanded).toBe(false);
  });

  it("reveals the folded records as nested rows once the group is opened", () => {
    const rows = flattenSessionTimeline(timeline, {
      collapsedBlockIds: new Set(),
      expandedEntryGroupIds: new Set(["entry-group:tool-1"]),
      debugEntriesVisible: true,
    });

    expect(rows.map((row) => row.id)).toEqual([
      "entry:meta-1",
      "group:turn-1-user-1",
      "entry:user-1",
      "entry-group:tool-1",
      "entry:tool-1",
      "entry:event-1",
      "entry:assistant-1",
    ]);
    expect(
      rows
        .filter((row) => row.kind === "entry")
        .map((row) => `${row.id}:${row.nested}`),
    ).toEqual([
      "entry:meta-1:false",
      "entry:user-1:false",
      "entry:tool-1:true",
      "entry:event-1:true",
      "entry:assistant-1:false",
    ]);
  });

  it("never folds what the user wrote or the agent replied", () => {
    expect(foldsIntoTimelineEntryGroup(entries[1])).toBe(false);
    expect(foldsIntoTimelineEntryGroup(entries[4])).toBe(false);
    expect(foldsIntoTimelineEntryGroup(entries[2])).toBe(true);
    expect(foldsIntoTimelineEntryGroup(entries[3])).toBe(true);
    // 后端把报错事件提升为 primary，它必须留在时间线主干上
    expect(
      foldsIntoTimelineEntryGroup({ ...entries[3], importance: "primary" }),
    ).toBe(false);
  });

  it("summarizes a group by what it actually contains", () => {
    expect(
      summarizeGroupedEntries([entries[2], entries[2], entries[3]]),
    ).toBe("工具调用 2 · 事件 1");
  });

  it("hides the structural debug records until the user opens the switch", () => {
    expect(
      flattenSessionTimeline(timeline, {
        collapsedBlockIds: new Set(),
        expandedEntryGroupIds: new Set(),
        debugEntriesVisible: false,
      }).map((row) => row.id),
    ).toEqual([
      "group:turn-1-user-1",
      "entry:user-1",
      "entry:tool-1",
      "entry:assistant-1",
    ]);
  });

  it("keeps every non-debug entry visible while the switch is off", () => {
    expect(showsTimelineEntry(entries[0], false)).toBe(false);
    expect(showsTimelineEntry(entries[0], true)).toBe(true);
    expect(showsTimelineEntry(entries[2], false)).toBe(true);
    expect(showsTimelineEntry(entries[4], false)).toBe(true);
  });

  it("emphasizes only what the user wrote and what the agent replied", () => {
    expect(resolveTimelineEntryEmphasis(entries[1])).toBe("user");
    expect(resolveTimelineEntryEmphasis(entries[4])).toBe("assistant");
    expect(resolveTimelineEntryEmphasis(entries[0])).toBe("muted");
    expect(resolveTimelineEntryEmphasis(entries[2])).toBe("muted");
    expect(resolveTimelineEntryEmphasis(entries[3])).toBe("muted");
    expect(
      resolveTimelineEntryEmphasis({ ...entries[3], category: "reasoning" }),
    ).toBe("muted");
  });

  it("shows normalized content only for primary entries", () => {
    expect(showsTimelineEntryPreview("primary")).toBe(true);
    expect(showsTimelineEntryPreview("detail")).toBe(false);
    expect(showsTimelineEntryPreview("debug")).toBe(false);
  });

  it("collapses only the selected turn and keeps its summary header", () => {
    expect(
      flattenSessionTimeline(timeline, {
        collapsedBlockIds: new Set(["turn-1-user-1"]),
        expandedEntryGroupIds: new Set(),
        debugEntriesVisible: true,
      }).map((row) => row.id),
    ).toEqual(["entry:meta-1", "group:turn-1-user-1"]);
  });

  it("pins the current turn after its original header leaves the viewport", () => {
    const rows = flattenSessionTimeline(timeline, {
      collapsedBlockIds: new Set(),
      expandedEntryGroupIds: new Set(),
      debugEntriesVisible: true,
    });
    const items = rows.map((_row, index) => ({
      index,
      start: index * 100,
      height: 100,
    }));

    expect(resolvePinnedTurnHeader({ rows, items, scrollTop: 100 })).toBeNull();
    expect(
      resolvePinnedTurnHeader({ rows, items, scrollTop: 200 })?.block.id,
    ).toBe("turn-1-user-1");
    expect(
      resolvePinnedTurnHeader({ rows, items, scrollTop: 350 })?.block.id,
    ).toBe("turn-1-user-1");
  });

  it("does not pin a turn while the viewport is still in session context", () => {
    const rows = flattenSessionTimeline(timeline, {
      collapsedBlockIds: new Set(),
      expandedEntryGroupIds: new Set(),
      debugEntriesVisible: true,
    });
    const items = rows.map((_row, index) => ({
      index,
      start: index * 100,
      height: 100,
    }));

    expect(resolvePinnedTurnHeader({ rows, items, scrollTop: 50 })).toBeNull();
  });

});
