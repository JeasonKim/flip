import { useCallback, useState } from "react";
import type { SessionAgentId, SessionMeta } from "../types/profile";
import { useSessions } from "../hooks/useSessions";
import { sessionIdentityKey } from "../lib/sessionRefresh";
import { purgeSessions } from "../lib/invoke";
import SessionList from "../components/session/SessionList";
import SessionDetail from "../components/session/SessionDetail";

function SessionPage() {
  const {
    sessions,
    loading,
    hasMore,
    agentFilter,
    setAgentFilter,
    loadMore,
    refresh,
  } = useSessions();

  const [selected, setSelected] = useState<SessionMeta | null>(null);
  const [purgeResult, setPurgeResult] = useState<string | null>(null);
  const [confirmingPurge, setConfirmingPurge] = useState<number | null>(null);

  const handlePurge = useCallback(
    async (days: number) => {
      if (confirmingPurge !== days) {
        setConfirmingPurge(days);
        setTimeout(() => setConfirmingPurge(null), 3000);
        return;
      }
      setConfirmingPurge(null);
      try {
        const count = await purgeSessions(days);
        setPurgeResult(`已删除 ${count} 个会话`);
        setTimeout(() => setPurgeResult(null), 3000);
        refresh();
      } catch (e) {
        setPurgeResult(`删除失败: ${e}`);
        setTimeout(() => setPurgeResult(null), 3000);
      }
    },
    [confirmingPurge, refresh],
  );

  return (
    <div className="h-screen bg-gray-900 text-white flex flex-col">
      {/* 顶部栏：Agent 切换 + 清理按钮 */}
      <div className="shrink-0 border-b border-white/10 px-4 py-3 flex items-center gap-3">
        {/* Agent 切换 */}
        <div className="flex items-center gap-1 bg-white/5 rounded-lg p-0.5">
          {(["all", "claude", "codex", "opencode"] as const).map((val) => {
            const isActive =
              val === "all" ? !agentFilter : agentFilter === val;
            const label =
              val === "all"
                ? "All"
                : val === "claude"
                  ? "Claude"
                  : val === "codex"
                    ? "Codex"
                    : "OpenCode";
            return (
              <button
                key={val}
                onClick={() =>
                  setAgentFilter(
                    val === "all" ? undefined : (val as SessionAgentId),
                  )
                }
                className={`px-3 py-1 text-xs rounded-md transition-colors ${
                  isActive
                    ? "bg-white/15 text-white"
                    : "text-gray-400 hover:text-gray-200"
                }`}
              >
                {label}
              </button>
            );
          })}
        </div>

        <div className="flex-1" />

        <div className="flex items-center gap-2 shrink-0">
          {purgeResult && (
            <span className="text-[11px] text-emerald-400 animate-pulse">
              {purgeResult}
            </span>
          )}
          <button
            onClick={() => handlePurge(7)}
            className={`text-[11px] transition-colors ${
              confirmingPurge === 7
                ? "text-red-400 font-semibold"
                : "text-gray-500 hover:text-red-400"
            }`}
            title="删除最后活跃早于 7 天前的会话"
          >
            {confirmingPurge === 7 ? "确认删除?" : "清理 7d+"}
          </button>
          <button
            onClick={() => handlePurge(30)}
            className={`text-[11px] transition-colors ${
              confirmingPurge === 30
                ? "text-red-400 font-semibold"
                : "text-gray-500 hover:text-red-400"
            }`}
            title="删除最后活跃早于 30 天前的会话"
          >
            {confirmingPurge === 30 ? "确认删除?" : "清理 30d+"}
          </button>
        </div>
      </div>

      {/* 主体区域：左列表 + 右详情 */}
      <div className="flex-1 flex overflow-hidden">
        {/* 会话列表 */}
        <div className="w-80 shrink-0 border-r border-white/10 p-2 overflow-y-auto">
          {loading ? (
            <p className="text-sm text-gray-500 animate-pulse p-3">
              Scanning sessions...
            </p>
          ) : sessions.length === 0 ? (
            <p className="text-sm text-gray-500 italic p-3">
              No sessions found
            </p>
          ) : (
            <SessionList
              sessions={sessions}
              selectedKey={
                selected ? sessionIdentityKey(selected) : null
              }
              hasMore={hasMore}
              onSelect={setSelected}
              onLoadMore={loadMore}
            />
          )}
        </div>

        {/* 消息详情 */}
        <div className="flex-1 p-4 overflow-hidden">
          {selected ? (
            <SessionDetail key={sessionIdentityKey(selected)} session={selected} />
          ) : (
            <div className="flex items-center justify-center h-full text-gray-500 text-sm">
              Select a session to view messages
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

export default SessionPage;
