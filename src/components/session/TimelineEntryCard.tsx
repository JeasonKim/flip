import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import {
  Children,
  isValidElement,
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent,
  type ReactNode,
} from "react";
import type { Components } from "react-markdown";
import type { SessionTimelineEntry } from "../../types/profile";
import type { TimelineEntryEmphasis } from "../../lib/sessionTimeline";
import {
  loadSessionEntryRaw,
  openSessionResource,
} from "../../lib/invoke";
import {
  formatSessionRawValue,
  sessionRawValueHasLargeStrings,
} from "../../lib/sessionRawView";
import { preserveSessionResourceHref } from "../../lib/sessionResource";
import {
  resolveTimelineEntryEmphasis,
  showsTimelineEntryPreview,
} from "../../lib/sessionTimeline";
import {
  isMermaidDiagramSource,
  normalizeMarkdownDiagrams,
} from "../../lib/markdownDiagrams";
import MermaidBlock from "./MermaidBlock";

const emphasisStyle: Record<TimelineEntryEmphasis, TimelineEmphasisStyle> = {
  user: {
    background: "bg-blue-500/10 border-blue-500/20 hover:bg-blue-500/15",
    labelColor: "text-blue-400",
  },
  assistant: {
    background: "bg-violet-500/10 border-violet-500/20 hover:bg-violet-500/15",
    labelColor: "text-violet-400",
  },
  muted: {
    background: "bg-gray-500/10 border-gray-500/20 hover:bg-gray-500/15",
    labelColor: "text-gray-400",
  },
};

interface TimelineEmphasisStyle {
  background: string;
  labelColor: string;
}

interface TimelineEntryCardProps {
  entry: SessionTimelineEntry;
  agent: string;
  sourcePath: string;
  projectDir: string | null;
  onInspectEntry: () => void;
}

interface CodeElementProps {
  className?: string;
  children?: ReactNode;
}

interface MarkdownCodeBlock {
  code: string;
  language: string | null;
}

interface LoadedRawSnapshot {
  fingerprint: string;
  value: unknown;
}

interface RawLoadRequest {
  id: number;
  fingerprint: string;
}

function TimelineEntryCard({
  entry,
  agent,
  sourcePath,
  projectDir,
  onInspectEntry,
}: TimelineEntryCardProps) {
  const style = emphasisStyle[resolveTimelineEntryEmphasis(entry)];
  const [rawExpanded, setRawExpanded] = useState(false);
  const [rawSnapshot, setRawSnapshot] = useState<LoadedRawSnapshot | null>(null);
  const [rawLoading, setRawLoading] = useState(false);
  const [rawError, setRawError] = useState<string | null>(null);
  const [revealLargeStrings, setRevealLargeStrings] = useState(false);
  const cardRef = useRef<HTMLElement>(null);
  const rawLoadInFlightRef = useRef<RawLoadRequest | null>(null);
  const rawLoadSequenceRef = useRef(0);
  const entryFingerprint = timelineEntryFingerprint(entry);
  const previewVisible = showsTimelineEntryPreview(entry.importance);
  // 没有正文的条目本来就只有一行标题，用与分组行一致的紧凑排版，展开后能一眼看到更多条
  const compact = !previewVisible;

  const openResource = useCallback(
    async (href: string) => {
      try {
        await openSessionResource(href, projectDir ?? undefined);
      } catch (error) {
        console.warn(
          `[sessions] resource open failed agent=${agent} source=${sourcePath} entry=${entry.id} href=${JSON.stringify(href)} project_dir=${JSON.stringify(projectDir)}`,
          error,
        );
      }
    },
    [agent, entry.id, projectDir, sourcePath],
  );

  const markdownComponents = useMemo<Components>(
    () => ({
      pre({ children, ...props }) {
        const codeBlock = extractMarkdownCodeBlock(children);
        if (codeBlock && shouldRenderMermaidDiagram(codeBlock)) {
          return <MermaidBlock chart={codeBlock.code} />;
        }
        return <pre {...props}>{children}</pre>;
      },
      a({ href, children, onClick: _ignoredOnClick, ...props }) {
        const safeHref = preserveSessionResourceHref(href ?? "");
        const handleResourceClick = (event: MouseEvent<HTMLAnchorElement>) => {
          event.preventDefault();
          if (safeHref) {
            void openResource(safeHref);
          }
        };
        return (
          <a {...props} href={safeHref || undefined} onClick={handleResourceClick}>
            {children}
          </a>
        );
      },
    }),
    [openResource],
  );

  const revealRawValue = useCallback(async () => {
    if (rawLoadInFlightRef.current?.fingerprint === entryFingerprint) {
      return;
    }

    const request: RawLoadRequest = {
      id: rawLoadSequenceRef.current + 1,
      fingerprint: entryFingerprint,
    };
    rawLoadSequenceRef.current = request.id;
    rawLoadInFlightRef.current = request;
    setRawLoading(true);
    setRawError(null);
    try {
      const value = await loadSessionEntryRaw(
        agent,
        sourcePath,
        entry.raw_reference,
      );
      if (rawLoadInFlightRef.current?.id === request.id) {
        setRawSnapshot({ fingerprint: entryFingerprint, value });
      }
    } catch (error) {
      console.warn(
        `[sessions] entry raw reveal failed agent=${agent} source=${sourcePath} entry=${entry.id} section=${entry.raw_reference.section} index=${entry.raw_reference.index}`,
        error,
      );
      if (rawLoadInFlightRef.current?.id === request.id) {
        setRawError(String(error));
      }
    } finally {
      if (rawLoadInFlightRef.current?.id === request.id) {
        rawLoadInFlightRef.current = null;
        setRawLoading(false);
      }
    }
  }, [agent, entry, entryFingerprint, sourcePath]);

  useEffect(() => {
    setRevealLargeStrings(false);
    if (
      rawExpanded &&
      rawSnapshot?.fingerprint !== entryFingerprint &&
      rawLoadInFlightRef.current?.fingerprint !== entryFingerprint
    ) {
      void revealRawValue();
    }
  }, [entryFingerprint, rawExpanded, rawSnapshot, revealRawValue]);

  const toggleRawRecord = useCallback(() => {
    onInspectEntry();
    setRawExpanded((expanded) => !expanded);
  }, [onInspectEntry]);

  // 整张卡片都是查看原始数据的热区，但要让文本选中、链接和内嵌按钮各自保留原有行为
  const inspectRawRecord = useCallback(
    (event: MouseEvent<HTMLElement>) => {
      const target = event.target as HTMLElement | null;
      if (target?.closest("a, button, input, textarea, select")) return;

      // 只有正文里真的划出了选区才让路；卡片其他区域禁用选中，连点不会被误判成划词
      const selection = window.getSelection();
      const selectionAnchor = selection?.isCollapsed ? null : selection?.anchorNode;
      if (selectionAnchor && cardRef.current?.contains(selectionAnchor)) return;

      toggleRawRecord();
    },
    [toggleRawRecord],
  );

  const loadedRawSnapshot =
    rawSnapshot?.fingerprint === entryFingerprint ? rawSnapshot : null;
  const loadedRawValue = loadedRawSnapshot?.value;
  const rawText = useMemo(
    () =>
      loadedRawSnapshot === null
        ? ""
        : formatSessionRawValue(loadedRawValue, revealLargeStrings),
    [loadedRawSnapshot, loadedRawValue, revealLargeStrings],
  );
  const hasLargeStrings = useMemo(
    () =>
      loadedRawSnapshot !== null &&
      sessionRawValueHasLargeStrings(loadedRawValue),
    [loadedRawSnapshot, loadedRawValue],
  );

  return (
    <article
      ref={cardRef}
      onClick={inspectRawRecord}
      className={`cursor-pointer select-none border transition-colors ${
        compact ? "rounded-md px-3 py-1.5" : "rounded-lg p-3"
      } ${style.background}`}
    >
      <div
        className={`flex items-center gap-2 ${previewVisible ? "mb-1.5" : ""}`}
        title={`点击条目查看原始数据：${entry.raw_reference.section} #${entry.raw_reference.index}`}
      >
        <span
          className={`min-w-0 truncate ${
            compact ? "text-[11px] font-normal" : "text-xs font-semibold"
          } ${style.labelColor}`}
        >
          {entry.title}
        </span>
        <span className="flex-1" />
        <span className="text-[10px] tabular-nums text-gray-500">
          {formatTimestamp(entry.timestamp)}
        </span>
        <button
          type="button"
          onClick={toggleRawRecord}
          aria-expanded={rawExpanded}
          aria-label={rawExpanded ? "收起原始数据" : "查看原始数据"}
          className={`text-[10px] leading-none transition-colors ${
            rawExpanded
              ? "text-gray-300"
              : "text-gray-600 hover:text-gray-300"
          }`}
        >
          {rawExpanded ? "▾" : "▸"}
        </button>
      </div>

      {previewVisible && (
        <div className="select-text">
          <TimelineEntryPreview
            entry={entry}
            markdownComponents={markdownComponents}
          />
        </div>
      )}

      {rawExpanded && (
        <section
          onClick={(event) => event.stopPropagation()}
          className={`cursor-auto select-text overflow-hidden rounded-md border border-white/10 bg-black/25 ${
            compact ? "mt-1.5" : "mt-3"
          }`}
        >
          <div className="flex items-center gap-2 border-b border-white/10 px-3 py-1.5">
            <span className="text-[10px] font-semibold uppercase tracking-wide text-gray-400">
              原始数据
            </span>
            <span className="font-mono text-[10px] text-gray-600">
              {entry.raw_reference.section} #{entry.raw_reference.index}
            </span>
            {entry.raw_reference.record_id && (
              <span className="min-w-0 truncate font-mono text-[10px] text-gray-600">
                {entry.raw_reference.record_id}
              </span>
            )}
            <span className="flex-1" />
            {hasLargeStrings && (
              <button
                type="button"
                onClick={() => setRevealLargeStrings((revealed) => !revealed)}
                className="text-[10px] text-amber-400/80 hover:text-amber-300"
              >
                {revealLargeStrings ? "隐藏大字段" : "显示完整大字段"}
              </button>
            )}
          </div>
          {rawLoading && !loadedRawSnapshot ? (
            <p className="px-3 py-3 text-xs text-gray-500 animate-pulse">
              正在读取原始数据…
            </p>
          ) : rawError ? (
            <div className="flex items-center gap-3 px-3 py-3">
              <p className="min-w-0 flex-1 break-words text-xs text-red-400">
                {rawError}
              </p>
              <button
                type="button"
                onClick={() => void revealRawValue()}
                className="shrink-0 text-xs text-gray-300 hover:text-white"
              >
                重试
              </button>
            </div>
          ) : (
            <pre className="max-h-96 overflow-auto p-3 font-mono text-[11px] leading-relaxed text-gray-300 whitespace-pre">
              {rawText}
            </pre>
          )}
        </section>
      )}
    </article>
  );
}

export default memo(TimelineEntryCard);

interface TimelineEntryPreviewProps {
  entry: SessionTimelineEntry;
  markdownComponents: Components;
}

function TimelineEntryPreview({
  entry,
  markdownComponents,
}: TimelineEntryPreviewProps) {
  if (entry.category === "tool_call" || entry.category === "tool_result") {
    return (
      <pre className="max-h-48 overflow-hidden whitespace-pre-wrap break-words font-mono text-xs leading-relaxed text-gray-400">
        {entry.content || "[没有可预览内容]"}
      </pre>
    );
  }

  if (
    entry.category === "metadata" ||
    entry.category === "event" ||
    entry.category === "system" ||
    entry.category === "unknown"
  ) {
    return (
      <pre className="max-h-36 overflow-hidden whitespace-pre-wrap break-words font-mono text-[11px] leading-relaxed text-gray-500">
        {entry.content || "[没有可预览内容]"}
      </pre>
    );
  }

  return (
    <div className="prose prose-invert prose-sm max-w-none break-words prose-pre:bg-black/30 prose-pre:text-xs prose-code:text-xs">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeHighlight]}
        components={markdownComponents}
        urlTransform={preserveSessionResourceHref}
      >
        {normalizeMarkdownDiagrams(entry.content)}
      </ReactMarkdown>
    </div>
  );
}

function formatTimestamp(timestamp: number | null): string {
  if (!timestamp) return "";
  return new Date(timestamp).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function timelineEntryFingerprint(entry: SessionTimelineEntry): string {
  const contentTail = entry.content.slice(-80);
  return [
    entry.id,
    entry.category,
    entry.title,
    entry.timestamp ?? "na",
    entry.content.length,
    contentTail,
  ].join(":");
}

function shouldRenderMermaidDiagram(codeBlock: MarkdownCodeBlock): boolean {
  return (
    codeBlock.language === "mermaid" ||
    (codeBlock.language === null && isMermaidDiagramSource(codeBlock.code))
  );
}

function extractMarkdownCodeBlock(children: ReactNode): MarkdownCodeBlock | null {
  const child = Children.toArray(children)[0];
  if (!isValidElement<CodeElementProps>(child) || child.type !== "code") {
    return null;
  }

  const className = child.props.className ?? "";
  const language = className.match(/language-([^\s]+)/)?.[1] ?? null;
  const code = stringifyCodeChildren(child.props.children).replace(/\n$/, "");
  return { code, language };
}

function stringifyCodeChildren(children: ReactNode): string {
  if (Array.isArray(children)) {
    return children.map(stringifyCodeChildren).join("");
  }
  if (typeof children === "string" || typeof children === "number") {
    return String(children);
  }
  return "";
}
