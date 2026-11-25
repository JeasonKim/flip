import { useCallback, useState } from "react";
import type { SessionAgentId, SessionMeta } from "../types/profile";
import { useSessions } from "../hooks/useSessions";
import { sessionIdentityKey } from "../lib/sessionRefresh";
import { purgeSessions } from "../lib/invoke";
import {
  recallDebugEntryVisibility,
  rememberDebugEntryVisibility,
} from "../lib/sessionDebugEntryPreference";
import SessionList from "../components/session/SessionList";
import SessionDetail from "../components/session/SessionDetail";
import SessionSourceManager from "../components/session/SessionSourceManager";

function SessionPage() {
  const {
    sessions,
    sources,
    sourceError,
    error,
    loading,
    hasMore,
    agentFilter,
    sourceFilter,
    setAgentFilter,
    setSourceFilter,
    acceptSources,
    loadMore,
    refresh,
  } = useSessions();

  const [selected, setSelected] = useState<SessionMeta | null>(null);
  const [sourceManagerOpen, setSourceManagerOpen] = useState(false);
  const selectedSession = selected
    ? sessions.find((session) => sessionIdentityKey(session) === sessionIdentityKey(selected)) ?? selected
    : null;
  // 调试条目（system / metadata / event 等元记录）是全局阅读偏好，默认收起
  const [debugEntriesVisible, setDebugEntriesVisible] = useState(() =>
    recallDebugEntryVisibility(browserStorage()),
  );
  const [purgeResult, setPurgeResult] = useState<string | null>(null);
  const [confirmingPurge, setConfirmingPurge] = useState<number | null>(null);

  const toggleDebugEntries = useCallback(() => {
    setDebugEntriesVisible((visible) => {
      const nextVisible = !visible;
      rememberDebugEntryVisibility(browserStorage(), nextVisible);
      return nextVisible;
    });
  }, []);

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
      <div className="shrink-0 border-b border-white/10 px-4 py-3 flex flex-wrap items-center gap-3">
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
                onClick={() => {
                  setSelected(null);
                  setAgentFilter(
                    val === "all" ? undefined : (val as SessionAgentId),
                  );
                }}
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

        <div className="flex items-center gap-3 shrink-0">
          <button
            onClick={toggleDebugEntries}
            className={`rounded-md px-3 py-1 text-[11px] transition-colors ${
              debugEntriesVisible
                ? "bg-white/15 text-gray-100"
                : "bg-white/5 text-gray-500 hover:bg-white/10 hover:text-gray-300"
            }`}
            aria-pressed={debugEntriesVisible}
            title="显示 system / metadata / event 等原始记录条目，便于了解会话数据结构"
          >
            {debugEntriesVisible ? "● 调试条目" : "○ 调试条目"}
          </button>
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

      <div className="flex shrink-0 items-center gap-3 border-b border-white/10 px-4 py-2">
        <label className="flex min-w-0 items-center gap-2 text-xs text-gray-500">来源
          <select aria-label="筛选会话来源" value={sourceFilter ?? ""}
            disabled={Boolean(agentFilter && agentFilter !== "codex")}
            onChange={(event) => { setSelected(null); setSourceFilter(event.target.value || undefined); }}
            className="max-w-64 rounded-md border border-white/10 bg-gray-900 px-2 py-1.5 text-xs text-gray-300 outline-none focus:border-sky-500 disabled:opacity-40">
            <option value="">全部来源</option>
            {sources.filter((source) => source.enabled).map((source) => <option key={source.id} value={source.id}>{source.label}</option>)}
          </select>
        </label>
        <div className="flex-1" />
        <button type="button" onClick={() => setSourceManagerOpen(true)} disabled={Boolean(sourceError)}
          className="shrink-0 rounded-md px-2 py-1.5 text-xs text-gray-400 hover:bg-white/5 hover:text-gray-200 disabled:opacity-40">采集目录</button>
      </div>
      {(error || sourceError) && <div role="alert" className="flex shrink-0 items-center gap-3 border-b border-red-500/20 bg-red-500/5 px-4 py-2 text-xs text-red-300">
        <span className="min-w-0 flex-1 break-words">{error || sourceError}</span>
        <button type="button" onClick={() => void refresh()} className="shrink-0 underline">重试采集</button>
      </div>}

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
          {selectedSession ? (
            <SessionDetail
              key={sessionIdentityKey(selectedSession)}
              session={selectedSession}
              debugEntriesVisible={debugEntriesVisible}
            />
          ) : (
            <div className="flex items-center justify-center h-full text-gray-500 text-sm">
              Select a session to view messages
            </div>
          )}
        </div>
      </div>
      {sourceManagerOpen && <SessionSourceManager sources={sources} onClose={() => setSourceManagerOpen(false)} onSaved={(next) => { setSelected(null); acceptSources(next); }} />}
    </div>
  );
}

/// Tauri WebView 之外的渲染环境（如测试或 SSR）不保证有 localStorage
function browserStorage(): Storage | null {
  return typeof window === "undefined" ? null : window.localStorage;
}

export default SessionPage;
