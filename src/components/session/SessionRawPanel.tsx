import type { SessionMeta, SessionRawRecord } from "../../types/profile";
import { useSessionRawContent } from "../../hooks/useSessions";

interface SessionRawPanelProps {
  session: SessionMeta;
  active: boolean;
}

export default function SessionRawPanel({
  session,
  active,
}: SessionRawPanelProps) {
  const { rawContent, loading, error, refresh } = useSessionRawContent(
    session.agent,
    session.source_path,
    active,
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
        <div className="flex items-center gap-2 text-[11px] text-gray-400">
          <span className="uppercase font-bold text-gray-300">
            {rawContent.agent}
          </span>
          <span>{rawContent.records.length} records</span>
          {rawContent.truncated && (
            <span className="text-amber-400">已截断记录数</span>
          )}
          {loading && <span className="text-gray-500">Refreshing...</span>}
        </div>
        <p className="mt-1 text-[10px] text-gray-600 truncate font-mono">
          {rawContent.source_path}
        </p>
      </div>

      {rawContent.records.map((record) => (
        <RawRecordBlock
          key={`${record.section}-${record.index}`}
          record={record}
        />
      ))}
    </div>
  );
}

interface RawRecordBlockProps {
  record: SessionRawRecord;
}

function RawRecordBlock({ record }: RawRecordBlockProps) {
  return (
    <section className="rounded-md border border-white/10 bg-white/[0.03] overflow-hidden">
      <div className="flex items-center gap-2 border-b border-white/10 bg-white/[0.03] px-3 py-1.5">
        <span className="text-[11px] uppercase font-semibold text-gray-300">
          {record.section}
        </span>
        <span className="text-[10px] text-gray-600 font-mono">
          #{record.index}
        </span>
      </div>
      <pre className="max-h-[520px] overflow-auto p-3 text-[11px] leading-relaxed text-gray-300 font-mono whitespace-pre-wrap break-words">
        {stringifyRawValue(record.value)}
      </pre>
    </section>
  );
}

function stringifyRawValue(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2) ?? String(value);
  } catch (error) {
    console.warn("[sessions] failed to stringify raw record", error);
    return String(value);
  }
}
