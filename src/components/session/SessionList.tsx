import type { SessionMeta } from "../../types/profile";
import { sessionIdentityKey } from "../../lib/sessionRefresh";

const AGENT_BADGE: Record<string, { label: string; color: string }> = {
  claude: { label: "CC", color: "text-violet-400" },
  codex: { label: "CX", color: "text-emerald-400" },
  opencode: { label: "OC", color: "text-amber-400" },
};

function agentBadge(agent: string) {
  return AGENT_BADGE[agent] ?? { label: agent.slice(0, 2).toUpperCase(), color: "text-gray-400" };
}

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
  selectedKey: string | null;
  hasMore: boolean;
  onSelect: (session: SessionMeta) => void;
  onLoadMore: () => void;
}

export default function SessionList({
  sessions,
  selectedKey,
  hasMore,
  onSelect,
  onLoadMore,
}: SessionListProps) {
  return (
    <div className="flex flex-col h-full">
      <div className="flex-1 overflow-y-auto space-y-0.5">
        {sessions.map((s) => (
          <button
            key={sessionIdentityKey(s)}
            onClick={() => onSelect(s)}
            className={`w-full text-left px-3 py-2.5 rounded-lg transition-colors ${
              selectedKey === sessionIdentityKey(s)
                ? "bg-white/15 ring-1 ring-white/20"
                : "hover:bg-white/5"
            }`}
          >
            <div className="flex items-center gap-2">
              {/* Agent 标识 */}
              {(() => {
                const badge = agentBadge(s.agent);
                return (
                  <span
                    className={`text-[10px] uppercase font-bold tracking-wider shrink-0 ${badge.color}`}
                  >
                    {badge.label}
                  </span>
                );
              })()}
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
          </button>
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
