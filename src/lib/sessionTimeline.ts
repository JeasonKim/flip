import type {
  SessionTimeline,
  SessionTimelineBlock,
  SessionTimelineEntry,
  SessionTimelineImportance,
  SessionTokenUsage,
} from "../types/profile";
import type { VirtualRangeItem } from "./sessionVirtualRange";

export interface TokenUsageMetric {
  key: keyof SessionTokenUsage;
  label: string;
  value: string;
}

export interface SessionTimelineEntryRow {
  id: string;
  kind: "entry";
  category: string;
  importance: SessionTimelineImportance;
  title: string;
  role: string;
  timestamp: number | null;
  content: string;
  entry: SessionTimelineEntry;
  /// 是否属于某个折叠分组，用于在时间线上体现层级缩进
  nested: boolean;
}

export interface SessionTimelineEntryGroupRow {
  id: string;
  kind: "entry-group";
  summary: string;
  timestamp: number | null;
  entries: SessionTimelineEntry[];
  expanded: boolean;
  category: string;
  importance: SessionTimelineImportance;
  title: string;
  role: string;
  content: string;
}

export interface SessionTimelineGroupRow {
  id: string;
  kind: "group";
  category: string;
  importance: SessionTimelineImportance;
  title: string;
  role: string;
  timestamp: number | null;
  content: string;
  block: SessionTimelineBlock;
  expanded: boolean;
}

export type SessionTimelineDisplayRow =
  | SessionTimelineEntryRow
  | SessionTimelineEntryGroupRow
  | SessionTimelineGroupRow;

export interface SessionTimelineDisplayState {
  collapsedBlockIds: ReadonlySet<string>;
  expandedEntryGroupIds: ReadonlySet<string>;
  debugEntriesVisible: boolean;
}

/// 用户视角的关注度：只有本人发言和 Agent 回复需要醒目，其余一律弱化
export type TimelineEntryEmphasis = "user" | "assistant" | "muted";

export interface PinnedTurnHeaderInput {
  rows: SessionTimelineDisplayRow[];
  items: VirtualRangeItem[];
  scrollTop: number;
}

/// 少于两条时直接平铺，避免为了一条记录多点一次
const MIN_GROUPED_ENTRY_COUNT = 2;

const groupedCategoryLabels: Record<string, string> = {
  tool_call: "工具调用",
  tool_result: "工具结果",
  reasoning: "思考",
  system: "系统记录",
  metadata: "元数据",
  event: "事件",
  unknown: "未知记录",
};

interface TokenUsageMetricDefinition {
  key: keyof SessionTokenUsage;
  label: string;
  /// 细分指标为 0 时不占位，避免 Turn 头被恒为零的列撑满
  hidesWhenZero?: boolean;
}

const usageMetricDefinitions: ReadonlyArray<TokenUsageMetricDefinition> = [
  { key: "total_tokens", label: "Total" },
  { key: "input_tokens", label: "Input" },
  { key: "output_tokens", label: "Output" },
  { key: "thinking_tokens", label: "Thinking", hidesWhenZero: true },
  { key: "cache_read_tokens", label: "Read cache" },
  { key: "cache_write_tokens", label: "Write cache" },
  { key: "cache_write_1h_tokens", label: "Write 1h", hidesWhenZero: true },
  { key: "cache_write_5m_tokens", label: "Write 5m", hidesWhenZero: true },
];

/// 不满一千的量直接给原始数字，压成 0K 会让人误以为根本没读到数据
export function formatTokenCountK(tokens: number): string {
  if (tokens < 1000) return String(tokens);
  return `${Number((tokens / 1000).toFixed(2))}K`;
}

export function visibleTokenUsageMetrics(
  usage: SessionTokenUsage,
): TokenUsageMetric[] {
  return usageMetricDefinitions.flatMap((definition) => {
    const tokens = usage[definition.key];
    if (tokens === null || tokens === undefined) return [];
    if (tokens === 0 && definition.hidesWhenZero) return [];
    return [
      {
        key: definition.key,
        label: definition.label,
        value: formatTokenCountK(tokens),
      },
    ];
  });
}

export function flattenSessionTimeline(
  timeline: SessionTimeline,
  state: SessionTimelineDisplayState,
): SessionTimelineDisplayRow[] {
  const rows: SessionTimelineDisplayRow[] = [];
  const representedEntryIndexes = new Set<number>();

  for (const block of timeline.blocks) {
    const entryStart = Math.max(0, block.entry_start);
    const entryEnd = Math.min(timeline.entries.length, block.entry_end);
    const expanded = !state.collapsedBlockIds.has(block.id);

    if (block.kind === "turn") {
      const firstEntry = timeline.entries[entryStart];
      rows.push({
        id: `group:${block.id}`,
        kind: "group",
        category: "turn",
        importance: "primary",
        title: `Turn ${block.ordinal ?? "?"}`,
        role: block.status ?? "completed",
        timestamp: firstEntry?.timestamp ?? null,
        content: JSON.stringify({
          status: block.status,
          usage: block.usage,
          expanded,
        }),
        block,
        expanded,
      });
    }

    const blockEntries: SessionTimelineEntry[] = [];
    for (let index = entryStart; index < entryEnd; index += 1) {
      representedEntryIndexes.add(index);
      const entry = timeline.entries[index];
      if (!showsTimelineEntry(entry, state.debugEntriesVisible)) continue;
      blockEntries.push(entry);
    }
    if (block.kind !== "turn" || expanded) {
      rows.push(...timelineRowsWithFoldedRuns(blockEntries, state));
    }
  }

  // 契约异常时仍展示未分组原始记录，避免一次边界解析错误让真实数据从 UI 消失。
  if (representedEntryIndexes.size !== timeline.entries.length) {
    const missingEntries = timeline.entries.filter(
      (_entry, index) => !representedEntryIndexes.has(index),
    );
    const missingEntrySample = missingEntries
      .slice(0, 20)
      .map((entry) => entry.id)
      .join(",");
    console.warn(
      `[sessions] timeline blocks omitted entries count=${missingEntries.length} sample_ids=${missingEntrySample}; appended records in source index order`,
    );
    rows.push(
      ...timelineRowsWithFoldedRuns(
        missingEntries.filter((entry) =>
          showsTimelineEntry(entry, state.debugEntriesVisible),
        ),
        state,
      ),
    );
  }

  return rows;
}

/// 调试条目是理解会话数据结构用的元记录，默认收起，只在用户主动打开开关时进入时间线
export function showsTimelineEntry(
  entry: SessionTimelineEntry,
  debugEntriesVisible: boolean,
): boolean {
  return debugEntriesVisible || entry.importance !== "debug";
}

/// 工具调用、思考和调试记录常常连续出现，成串折叠起来才不会淹没真正的对话
export function foldsIntoTimelineEntryGroup(entry: SessionTimelineEntry): boolean {
  return (
    entry.importance !== "primary" &&
    resolveTimelineEntryEmphasis(entry) === "muted"
  );
}

export function summarizeGroupedEntries(
  entries: SessionTimelineEntry[],
): string {
  const countsByCategory = new Map<string, number>();
  for (const entry of entries) {
    countsByCategory.set(
      entry.category,
      (countsByCategory.get(entry.category) ?? 0) + 1,
    );
  }

  return [...countsByCategory]
    .map(
      ([category, count]) =>
        `${groupedCategoryLabels[category] ?? category} ${count}`,
    )
    .join(" · ");
}

export function resolveTimelineEntryEmphasis(
  entry: SessionTimelineEntry,
): TimelineEntryEmphasis {
  if (entry.category === "user") return "user";
  if (entry.category === "assistant") return "assistant";
  return "muted";
}

export function showsTimelineEntryPreview(
  importance: SessionTimelineImportance,
): boolean {
  return importance === "primary";
}

export function resolvePinnedTurnHeader(
  input: PinnedTurnHeaderInput,
): SessionTimelineGroupRow | null {
  const viewportTop = Math.max(0, input.scrollTop);
  const topItem = input.items.find(
    (item) => item.start + item.height > viewportTop,
  );
  if (!topItem) return null;

  for (let index = topItem.index; index >= 0; index -= 1) {
    const row = input.rows[index];
    if (row?.kind !== "group") continue;

    // 原始组头尚未越过视口顶部时直接使用原节点，避免同一组头出现两份。
    if (index === topItem.index && topItem.start >= viewportTop) {
      return null;
    }
    return row;
  }

  return null;
}

function timelineRowsWithFoldedRuns(
  entries: SessionTimelineEntry[],
  state: SessionTimelineDisplayState,
): SessionTimelineDisplayRow[] {
  const rows: SessionTimelineDisplayRow[] = [];
  let foldableRun: SessionTimelineEntry[] = [];

  const settleFoldableRun = () => {
    if (foldableRun.length === 0) return;

    if (foldableRun.length < MIN_GROUPED_ENTRY_COUNT) {
      rows.push(...foldableRun.map((entry) => entryRow(entry, false)));
      foldableRun = [];
      return;
    }

    const groupId = `entry-group:${foldableRun[0].id}`;
    const expanded = state.expandedEntryGroupIds.has(groupId);
    const summary = summarizeGroupedEntries(foldableRun);
    rows.push({
      id: groupId,
      kind: "entry-group",
      summary,
      timestamp: foldableRun[0].timestamp,
      entries: foldableRun,
      expanded,
      category: "entry-group",
      importance: "detail",
      title: summary,
      role: "system",
      content: JSON.stringify({
        entryIds: foldableRun.map((entry) => entry.id),
        expanded,
      }),
    });
    if (expanded) {
      rows.push(...foldableRun.map((entry) => entryRow(entry, true)));
    }
    foldableRun = [];
  };

  for (const entry of entries) {
    if (foldsIntoTimelineEntryGroup(entry)) {
      foldableRun.push(entry);
      continue;
    }
    settleFoldableRun();
    rows.push(entryRow(entry, false));
  }
  settleFoldableRun();

  return rows;
}

function entryRow(
  entry: SessionTimelineEntry,
  nested: boolean,
): SessionTimelineEntryRow {
  return {
    id: `entry:${entry.id}`,
    kind: "entry",
    category: entry.category,
    importance: entry.importance,
    title: entry.title,
    role: entry.role,
    timestamp: entry.timestamp,
    content: entry.content,
    entry,
    nested,
  };
}
