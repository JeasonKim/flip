import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { RefObject } from "react";
import type {
  SessionTimeline,
  SessionTimelineBlock,
} from "../../types/profile";
import {
  flattenSessionTimeline,
  resolvePinnedTurnHeader,
  visibleTokenUsageMetrics,
  type SessionTimelineDisplayRow,
  type SessionTimelineEntryGroupRow,
} from "../../lib/sessionTimeline";
import {
  resolveMeasuredHeightScrollOffset,
  resolveSessionVirtualRange,
  retainMeasuredHeightsForStableItems,
  type VirtualRangeItem,
} from "../../lib/sessionVirtualRange";
import TimelineEntryCard from "./TimelineEntryCard";

const ESTIMATED_ENTRY_HEIGHT = 120;
const ENTRY_OVERSCAN = 8;

interface VirtualTimelineProps {
  timeline: SessionTimeline;
  agent: string;
  sourcePath: string;
  projectDir: string | null;
  scrollRef: RefObject<HTMLDivElement | null>;
  debugEntriesVisible: boolean;
  followTail: boolean;
  onProgrammaticScroll: () => void;
  onInspectEntry: () => void;
}

export default function VirtualTimeline({
  timeline,
  agent,
  sourcePath,
  projectDir,
  scrollRef,
  debugEntriesVisible,
  followTail,
  onProgrammaticScroll,
  onInspectEntry,
}: VirtualTimelineProps) {
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(0);
  const [measuredHeights, setMeasuredHeights] = useState<Record<number, number>>(
    {},
  );
  const measuredHeightsRef = useRef<Record<number, number>>({});
  const previousRowsRef = useRef<SessionTimelineDisplayRow[]>([]);
  const [collapsedBlockIds, setCollapsedBlockIds] = useState<Set<string>>(
    () => new Set(),
  );
  // 连续的工具调用与调试记录默认折叠，展开状态只在本次浏览中保留
  const [expandedEntryGroupIds, setExpandedEntryGroupIds] = useState<
    Set<string>
  >(() => new Set());
  const rows = useMemo(
    () =>
      flattenSessionTimeline(timeline, {
        collapsedBlockIds,
        expandedEntryGroupIds,
        debugEntriesVisible,
      }),
    [collapsedBlockIds, debugEntriesVisible, expandedEntryGroupIds, timeline],
  );

  const synchronizeViewport = useCallback(() => {
    const scrollElement = scrollRef.current;
    if (!scrollElement) return;
    setScrollTop(scrollElement.scrollTop);
    setViewportHeight(scrollElement.clientHeight);
  }, [scrollRef]);

  const scrollToMeasuredBottom = useCallback(() => {
    const scrollElement = scrollRef.current;
    if (!scrollElement) return;
    onProgrammaticScroll();
    scrollElement.scrollTop = scrollElement.scrollHeight;
    setScrollTop(scrollElement.scrollTop);
    setViewportHeight(scrollElement.clientHeight);
  }, [onProgrammaticScroll, scrollRef]);

  useEffect(() => {
    const retainedHeights = retainMeasuredHeightsForStableItems({
      currentHeights: measuredHeightsRef.current,
      previousItems: previousRowsRef.current,
      nextItems: rows,
    });
    measuredHeightsRef.current = retainedHeights;
    previousRowsRef.current = rows;
    setMeasuredHeights(retainedHeights);
    requestAnimationFrame(synchronizeViewport);
  }, [rows, synchronizeViewport]);

  useEffect(() => {
    const currentBlockIds = new Set(timeline.blocks.map((block) => block.id));
    setCollapsedBlockIds((current) => {
      const retained = new Set(
        [...current].filter((blockId) => currentBlockIds.has(blockId)),
      );
      return retained.size === current.size ? current : retained;
    });
  }, [timeline.blocks]);

  useEffect(() => {
    const scrollElement = scrollRef.current;
    if (!scrollElement) return;
    synchronizeViewport();
    scrollElement.addEventListener("scroll", synchronizeViewport, {
      passive: true,
    });
    const resizeObserver = new ResizeObserver(synchronizeViewport);
    resizeObserver.observe(scrollElement);
    return () => {
      scrollElement.removeEventListener("scroll", synchronizeViewport);
      resizeObserver.disconnect();
    };
  }, [scrollRef, synchronizeViewport]);

  const virtualRange = useMemo(
    () =>
      resolveSessionVirtualRange({
        itemCount: rows.length,
        scrollTop,
        viewportHeight,
        estimatedItemHeight: ESTIMATED_ENTRY_HEIGHT,
        overscan: ENTRY_OVERSCAN,
        measuredHeights,
      }),
    [measuredHeights, rows.length, scrollTop, viewportHeight],
  );
  const pinnedTurnHeader = useMemo(
    () =>
      resolvePinnedTurnHeader({
        rows,
        items: virtualRange.items,
        scrollTop,
      }),
    [rows, scrollTop, virtualRange.items],
  );

  useLayoutEffect(() => {
    if (!followTail) return;
    scrollToMeasuredBottom();
    const frame = requestAnimationFrame(scrollToMeasuredBottom);
    return () => cancelAnimationFrame(frame);
  }, [
    rows.length,
    followTail,
    scrollToMeasuredBottom,
    virtualRange.totalHeight,
  ]);

  const recordMeasuredHeight = useCallback(
    (item: VirtualRangeItem, measuredHeight: number) => {
      const normalizedHeight = Math.max(1, measuredHeight);
      const currentHeights = measuredHeightsRef.current;
      const previousHeight = currentHeights[item.index] ?? item.height;
      if (Math.abs(previousHeight - normalizedHeight) < 1) return;

      const scrollElement = scrollRef.current;
      const scrollOffset = resolveMeasuredHeightScrollOffset({
        followTail,
        itemStart: item.start,
        previousHeight,
        nextHeight: normalizedHeight,
        scrollTop: scrollElement?.scrollTop ?? scrollTop,
      });
      const nextHeights = {
        ...currentHeights,
        [item.index]: normalizedHeight,
      };
      measuredHeightsRef.current = nextHeights;
      setMeasuredHeights(nextHeights);

      if (scrollElement && scrollOffset !== 0) {
        onProgrammaticScroll();
        scrollElement.scrollTop += scrollOffset;
        setScrollTop(scrollElement.scrollTop);
        setViewportHeight(scrollElement.clientHeight);
      }
    },
    [followTail, onProgrammaticScroll, scrollRef, scrollTop],
  );

  const toggleEntryGroup = useCallback(
    (groupId: string) => {
      onInspectEntry();
      setExpandedEntryGroupIds((current) => {
        const next = new Set(current);
        if (next.has(groupId)) {
          next.delete(groupId);
        } else {
          next.add(groupId);
        }
        return next;
      });
    },
    [onInspectEntry],
  );

  const toggleTurn = useCallback(
    (blockId: string) => {
      onInspectEntry();
      setCollapsedBlockIds((current) => {
        const next = new Set(current);
        if (next.has(blockId)) {
          next.delete(blockId);
        } else {
          next.add(blockId);
        }
        return next;
      });
    },
    [onInspectEntry],
  );

  return (
    <div
      className="relative w-full"
      style={{ height: `${virtualRange.totalHeight}px` }}
    >
      {virtualRange.items.map((item) => {
        const row = rows[item.index];
        return (
          <VirtualTimelineRow
            key={row.id}
            item={item}
            row={row}
            agent={agent}
            sourcePath={sourcePath}
            projectDir={projectDir}
            onMeasure={recordMeasuredHeight}
            onInspectEntry={onInspectEntry}
            onToggleTurn={toggleTurn}
            onToggleEntryGroup={toggleEntryGroup}
          />
        );
      })}
      {pinnedTurnHeader && (
        <div className="sticky top-0 z-30 pointer-events-none">
          <div className="pointer-events-auto shadow-xl shadow-black/30">
            <TimelineTurnHeader
              block={pinnedTurnHeader.block}
              expanded={pinnedTurnHeader.expanded}
              pinned
              onToggleTurn={toggleTurn}
            />
          </div>
        </div>
      )}
    </div>
  );
}

interface VirtualTimelineRowProps {
  item: VirtualRangeItem;
  row: SessionTimelineDisplayRow;
  agent: string;
  sourcePath: string;
  projectDir: string | null;
  onMeasure: (item: VirtualRangeItem, height: number) => void;
  onInspectEntry: () => void;
  onToggleTurn: (blockId: string) => void;
  onToggleEntryGroup: (groupId: string) => void;
}

function VirtualTimelineRow({
  item,
  row,
  agent,
  sourcePath,
  projectDir,
  onMeasure,
  onInspectEntry,
  onToggleTurn,
  onToggleEntryGroup,
}: VirtualTimelineRowProps) {
  const rowRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const rowElement = rowRef.current;
    if (!rowElement) return;
    const measureRow = () => {
      onMeasure(item, rowElement.getBoundingClientRect().height);
    };
    measureRow();
    const resizeObserver = new ResizeObserver(measureRow);
    resizeObserver.observe(rowElement);
    return () => resizeObserver.disconnect();
  }, [item, onMeasure]);

  const nestedRow = row.kind === "entry" && row.nested;

  return (
    <div
      ref={rowRef}
      className={`absolute left-0 right-1 ${nestedRow ? "py-0.5" : "py-1"}`}
      style={{ transform: `translateY(${item.start}px)` }}
    >
      {row.kind === "group" ? (
        <TimelineTurnHeader
          block={row.block}
          expanded={row.expanded}
          onToggleTurn={onToggleTurn}
        />
      ) : row.kind === "entry-group" ? (
        <TimelineEntryGroupHeader
          row={row}
          onToggleEntryGroup={onToggleEntryGroup}
        />
      ) : (
        <div className={row.nested ? "border-l border-white/10 pl-2 ml-1" : ""}>
          <TimelineEntryCard
            entry={row.entry}
            agent={agent}
            sourcePath={sourcePath}
            projectDir={projectDir}
            onInspectEntry={onInspectEntry}
          />
        </div>
      )}
    </div>
  );
}

interface TimelineEntryGroupHeaderProps {
  row: SessionTimelineEntryGroupRow;
  onToggleEntryGroup: (groupId: string) => void;
}

function TimelineEntryGroupHeader({
  row,
  onToggleEntryGroup,
}: TimelineEntryGroupHeaderProps) {
  return (
    <button
      type="button"
      onClick={() => onToggleEntryGroup(row.id)}
      aria-expanded={row.expanded}
      className="flex w-full select-none items-center gap-2 rounded-lg border border-white/5 bg-white/[0.02] px-3 py-1.5 text-left transition-colors hover:bg-white/[0.06]"
    >
      <span className="w-3 shrink-0 text-[10px] text-gray-600">
        {row.expanded ? "▾" : "▸"}
      </span>
      <span className="min-w-0 flex-1 truncate text-[11px] text-gray-500">
        {row.summary}
      </span>
      <span className="shrink-0 text-[10px] text-gray-600">
        {row.entries.length} 条
      </span>
    </button>
  );
}

interface TimelineTurnHeaderProps {
  block: SessionTimelineBlock;
  expanded: boolean;
  pinned?: boolean;
  onToggleTurn: (blockId: string) => void;
}

function TimelineTurnHeader({
  block,
  expanded,
  pinned = false,
  onToggleTurn,
}: TimelineTurnHeaderProps) {
  const metrics = block.usage ? visibleTokenUsageMetrics(block.usage) : [];

  return (
    <button
      type="button"
      onClick={() => onToggleTurn(block.id)}
      className={`w-full flex items-center gap-2.5 px-3 py-2 text-left transition-colors ${
        pinned
          ? "rounded-none border-x-0 border-t-0 border-b border-white/15 bg-gray-900/95 hover:bg-gray-800/95 backdrop-blur"
          : "rounded-lg border border-white/10 bg-white/[0.04] hover:bg-white/[0.07]"
      }`}
      aria-expanded={expanded}
      title={expanded ? "收起这个 Turn" : "展开这个 Turn"}
    >
      <span className="w-3 shrink-0 text-[10px] text-gray-500">
        {expanded ? "▼" : "▶"}
      </span>
      <span className="shrink-0 text-xs font-semibold text-gray-200">
        Turn {block.ordinal ?? "?"}
      </span>
      {block.status === "running" ? (
        <span className="inline-flex shrink-0 items-center gap-1.5 text-[11px] text-amber-300">
          <span className="h-1.5 w-1.5 rounded-full bg-amber-300 animate-pulse" />
          调用中
        </span>
      ) : metrics.length > 0 ? (
        <span className="flex min-w-0 flex-wrap items-center gap-x-2.5 gap-y-1 text-[11px] text-gray-400">
          {metrics.map((metric) => (
            <span key={metric.key} className="whitespace-nowrap">
              <span className="text-gray-500">{metric.label}</span>{" "}
              <span className="font-mono text-gray-300">{metric.value}</span>
            </span>
          ))}
        </span>
      ) : (
        <span className="text-[11px] text-gray-500">已完成</span>
      )}
    </button>
  );
}
