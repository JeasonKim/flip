import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { KeyboardEvent, ReactNode } from "react";
import type { SessionMeta, SessionRawRecord } from "../../types/profile";
import { useSessionRawContent } from "../../hooks/useSessions";
import {
  collectRawSearchMatches,
  resolveRawSearchCursor,
  type RawSearchDirection,
  type RawRecordSnapshot,
} from "../../lib/sessionRawSearch";

interface SessionRawPanelProps {
  session: SessionMeta;
  active: boolean;
}

export default function SessionRawPanel({
  session,
  active,
}: SessionRawPanelProps) {
  const recordRefs = useRef<Map<string, HTMLElement>>(new Map());
  const rawTextCacheRef = useRef<Map<string, string>>(new Map());
  const pendingSearchDirectionRef = useRef<RawSearchDirection | null>(null);
  const [draftSearchQuery, setDraftSearchQuery] = useState("");
  const [committedSearchQuery, setCommittedSearchQuery] = useState("");
  const [currentMatchIndex, setCurrentMatchIndex] = useState(-1);
  const { rawContent, loading, error, refresh } = useSessionRawContent(
    session.agent,
    session.source_path,
    active,
  );

  useEffect(() => {
    rawTextCacheRef.current.clear();
  }, [rawContent]);

  const resolveRawRecordText = useCallback((record: SessionRawRecord) => {
    const key = rawRecordKey(record);
    const cached = rawTextCacheRef.current.get(key);
    if (cached !== undefined) {
      return cached;
    }

    const text = stringifyRawValue(record.value);
    rawTextCacheRef.current.set(key, text);
    return text;
  }, []);

  const registerRecordRef = useCallback<RegisterRawRecordRef>(
    (key, element) => {
      if (element) {
        recordRefs.current.set(key, element);
      } else {
        recordRefs.current.delete(key);
      }
    },
    [],
  );

  const rawRecordSnapshots = useMemo<RawRecordSnapshot[]>(() => {
    if (!committedSearchQuery) {
      return [];
    }

    return (
      rawContent?.records.map((record) => ({
        key: rawRecordKey(record),
        text: resolveRawRecordText(record),
      })) ?? []
    );
  }, [committedSearchQuery, rawContent, resolveRawRecordText]);

  const searchMatches = useMemo(
    () => collectRawSearchMatches(rawRecordSnapshots, committedSearchQuery),
    [committedSearchQuery, rawRecordSnapshots],
  );
  const activeMatch = searchMatches[currentMatchIndex] ?? null;
  const trimmedDraftSearchQuery = draftSearchQuery.trim();
  const searchCanRun =
    Boolean(trimmedDraftSearchQuery) &&
    (trimmedDraftSearchQuery !== committedSearchQuery ||
      searchMatches.length > 0);

  useEffect(() => {
    if (!committedSearchQuery || searchMatches.length === 0) {
      setCurrentMatchIndex(-1);
      return;
    }

    const pendingDirection = pendingSearchDirectionRef.current;
    pendingSearchDirectionRef.current = null;

    if (pendingDirection === "previous") {
      setCurrentMatchIndex(searchMatches.length - 1);
      return;
    }

    if (pendingDirection === "next") {
      setCurrentMatchIndex(0);
      return;
    }

    setCurrentMatchIndex((current) => {
      if (current >= 0 && current < searchMatches.length) {
        return current;
      }
      return 0;
    });
  }, [committedSearchQuery, searchMatches.length]);

  useEffect(() => {
    if (!activeMatch) {
      return;
    }

    recordRefs.current.get(activeMatch.recordKey)?.scrollIntoView({
      behavior: "smooth",
      block: "center",
    });
  }, [activeMatch]);

  const runSearch = useCallback(
    (direction: RawSearchDirection) => {
      const nextQuery = draftSearchQuery.trim();
      if (nextQuery !== committedSearchQuery) {
        pendingSearchDirectionRef.current = direction;
        setCommittedSearchQuery(nextQuery);
        return;
      }

      setCurrentMatchIndex((current) =>
        resolveRawSearchCursor(current, searchMatches.length, direction),
      );
    },
    [committedSearchQuery, draftSearchQuery, searchMatches.length],
  );

  const jumpSearchMatch = useCallback(
    (direction: RawSearchDirection) => {
      runSearch(direction);
    },
    [runSearch],
  );

  const handleSearchKeyDown = useCallback(
    (event: KeyboardEvent<HTMLInputElement>) => {
      if (event.key !== "Enter") {
        return;
      }

      event.preventDefault();
      jumpSearchMatch(event.shiftKey ? "previous" : "next");
    },
    [jumpSearchMatch],
  );

  if (loading && !rawContent) {
    return (
      <p className="text-sm text-gray-500 animate-pulse">
        Loading raw structure...
      </p>
    );
  }

  if (error) {
    return (
      <div className="space-y-3">
        <p className="text-sm text-red-400 bg-red-500/10 border border-red-500/20 rounded-md px-3 py-2">
          {error}
        </p>
        <button
          onClick={() => void refresh()}
          className="px-3 py-1.5 text-xs bg-white/10 hover:bg-white/20 text-gray-300 rounded-md transition-colors"
        >
          重试
        </button>
      </div>
    );
  }

  if (!rawContent || rawContent.records.length === 0) {
    return <p className="text-sm text-gray-500 italic">No raw records found</p>;
  }

  return (
    <div className="h-full overflow-y-auto pr-1 space-y-3">
      <div className="sticky top-0 z-10 bg-gray-900/95 border border-white/10 rounded-md px-3 py-2 backdrop-blur">
        <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex items-center gap-2 text-[11px] text-gray-400">
            <span className="uppercase font-bold text-gray-300">
              {rawContent.agent}
            </span>
            <span>{rawContent.records.length} records</span>
            {loading && <span className="text-gray-500">Refreshing...</span>}
          </div>
          <div className="flex items-center gap-1.5">
            <input
              value={draftSearchQuery}
              onChange={(event) => setDraftSearchQuery(event.target.value)}
              onKeyDown={handleSearchKeyDown}
              placeholder="搜索原始结构"
              className="h-7 w-44 rounded-md border border-white/10 bg-black/25 px-2 text-[11px] text-gray-200 outline-none placeholder:text-gray-600 focus:border-white/25"
            />
            <span className="w-14 text-center text-[10px] text-gray-500">
              {trimmedDraftSearchQuery !== committedSearchQuery
                ? "待搜索"
                : committedSearchQuery
                  ? searchMatches.length === 0
                    ? "0/0"
                    : `${currentMatchIndex + 1}/${searchMatches.length}`
                : ""}
            </span>
            <button
              onClick={() => jumpSearchMatch("previous")}
              disabled={!searchCanRun}
              className="h-7 w-7 rounded-md bg-white/10 text-xs text-gray-300 transition-colors hover:bg-white/20 disabled:cursor-not-allowed disabled:opacity-40"
              title="上一个匹配"
            >
              ↑
            </button>
            <button
              onClick={() => jumpSearchMatch("next")}
              disabled={!searchCanRun}
              className="h-7 w-7 rounded-md bg-white/10 text-xs text-gray-300 transition-colors hover:bg-white/20 disabled:cursor-not-allowed disabled:opacity-40"
              title="下一个匹配"
            >
              ↓
            </button>
          </div>
        </div>
        <p className="mt-1 break-all text-[10px] text-gray-600 font-mono">
          {rawContent.source_path}
        </p>
      </div>

      {rawContent.records.map((record) => (
        <RawRecordBlock
          key={`${record.section}-${record.index}`}
          record={record}
          recordKey={rawRecordKey(record)}
          highlightQuery={
            activeMatch?.recordKey === rawRecordKey(record)
              ? committedSearchQuery
              : ""
          }
          activeMatchIndex={
            activeMatch?.recordKey === rawRecordKey(record)
              ? activeMatch.matchIndex
              : null
          }
          registerRecordRef={registerRecordRef}
          resolveRawRecordText={resolveRawRecordText}
        />
      ))}
    </div>
  );
}

interface RawRecordBlockProps {
  record: SessionRawRecord;
  recordKey: string;
  highlightQuery: string;
  activeMatchIndex: number | null;
  registerRecordRef: RegisterRawRecordRef;
  resolveRawRecordText: ResolveRawRecordText;
}

type RegisterRawRecordRef = (key: string, element: HTMLElement | null) => void;
type ResolveRawRecordText = (record: SessionRawRecord) => string;

const RawRecordBlock = memo(function RawRecordBlock({
  record,
  recordKey,
  highlightQuery,
  activeMatchIndex,
  registerRecordRef,
  resolveRawRecordText,
}: RawRecordBlockProps) {
  const rawText = resolveRawRecordText(record);
  const isActiveRecord = activeMatchIndex !== null;

  return (
    <section
      ref={(element) => registerRecordRef(recordKey, element)}
      className={`rounded-md border bg-white/[0.03] overflow-hidden ${
        isActiveRecord ? "border-amber-400/60" : "border-white/10"
      }`}
    >
      <div className="flex items-center gap-2 border-b border-white/10 bg-white/[0.03] px-3 py-1.5">
        <span className="text-[11px] uppercase font-semibold text-gray-300">
          {record.section}
        </span>
        <span className="text-[10px] text-gray-600 font-mono">
          #{record.index}
        </span>
      </div>
      <pre className="overflow-visible p-3 text-[11px] leading-relaxed text-gray-300 font-mono whitespace-pre-wrap break-words">
        <HighlightedRawText
          text={rawText}
          query={highlightQuery}
          activeMatchIndex={activeMatchIndex}
        />
      </pre>
    </section>
  );
});

interface HighlightedRawTextProps {
  text: string;
  query: string;
  activeMatchIndex: number | null;
}

function HighlightedRawText({
  text,
  query,
  activeMatchIndex,
}: HighlightedRawTextProps) {
  const normalizedQuery = query.trim().toLowerCase();
  if (!normalizedQuery) {
    return <>{text}</>;
  }

  const normalizedText = text.toLowerCase();
  const parts: ReactNode[] = [];
  let cursor = 0;
  let partIndex = 0;

  while (cursor < text.length) {
    const matchIndex = normalizedText.indexOf(normalizedQuery, cursor);
    if (matchIndex === -1) {
      break;
    }

    if (matchIndex > cursor) {
      parts.push(
        <span key={`text-${partIndex}`}>{text.slice(cursor, matchIndex)}</span>,
      );
      partIndex += 1;
    }

    const endIndex = matchIndex + normalizedQuery.length;
    const isActiveMatch = activeMatchIndex === matchIndex;
    parts.push(
      <mark
        key={`match-${partIndex}`}
        className={
          isActiveMatch
            ? "rounded-sm bg-amber-400 px-0.5 text-black"
            : "rounded-sm bg-amber-400/25 px-0.5 text-amber-100"
        }
      >
        {text.slice(matchIndex, endIndex)}
      </mark>,
    );
    partIndex += 1;
    cursor = endIndex;
  }

  if (parts.length === 0) {
    return <>{text}</>;
  }

  if (cursor < text.length) {
    parts.push(<span key={`text-${partIndex}`}>{text.slice(cursor)}</span>);
  }

  return <>{parts}</>;
}

function rawRecordKey(record: SessionRawRecord): string {
  return `${record.section}-${record.index}`;
}

function stringifyRawValue(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2) ?? String(value);
  } catch (error) {
    console.warn("[sessions] failed to stringify raw record", error);
    return String(value);
  }
}
