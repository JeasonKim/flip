import { motion } from "framer-motion";
import type { SessionMeta } from "../../types/profile";

function formatDate(ts: number | null): string {
  if (!ts) return "—";
  const diffSec = Math.floor((Date.now() - ts) / 1000);
  if (diffSec < 60) return "刚刚";
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}分钟前`;
  const diffHour = Math.floor(diffMin / 60);
  if (diffHour < 24) return `${diffHour}小时前`;
  const diffDay = Math.floor(diffHour / 24);
  if (diffDay < 7) return `${diffDay}天前`;
  const d = new Date(ts);
  return `${d.getFullYear()}/${d.getMonth() + 1}/${d.getDate()}`;
}

interface SessionListProps {
  sessions: SessionMeta[];
  selectedId: string | null;
  hasMore: boolean;
  onSelect: (session: SessionMeta) => void;
  onLoadMore: () => void;
}

export default function SessionList({
  sessions,
  selectedId,
  hasMore,
  onSelect,
  onLoadMore,
}: SessionListProps) {
  return (
    <div className="flex flex-col h-full">
      <div className="flex-1 overflow-y-auto space-y-0.5">
        {sessions.map((s) => (
          <motion.button
            key={`${s.agent}-${s.session_id}`}
            layout
            onClick={() => onSelect(s)}
            className={`w-full text-left px-3 py-2.5 rounded-lg transition-colors ${
              selectedId === s.session_id
                ? "bg-white/15 ring-1 ring-white/20"
                : "hover:bg-white/5"
            }`}
          >
            <div className="flex items-center gap-2">
              {/* Agent 标识 */}
              <span
                className={`text-[10px] uppercase font-bold tracking-wider shrink-0 ${
                  s.agent === "claude" ? "text-violet-400" : "text-emerald-400"
                }`}
              >
                {s.agent === "claude" ? "CC" : "CX"}
              </span>
              {/* 标题 */}
              <span className="text-sm text-gray-200 truncate flex-1">
                {s.title}
              </span>
              {/* 时间 */}
              <span className="text-[10px] text-gray-500 tabular-nums shrink-0">
                {formatDate(s.last_active_at)}
              </span>
            </div>
            {/* 项目路径 */}
            {s.project_dir && (
              <p className="text-[10px] text-gray-500 truncate mt-0.5 pl-7">
                {s.project_dir}
              </p>
            )}
          </motion.button>
        ))}
      </div>

      {hasMore && (
        <button
          onClick={onLoadMore}
          className="mt-2 text-xs text-gray-400 hover:text-white transition-colors py-2"
        >
          Load more...
        </button>
      )}
    </div>
  );
}
