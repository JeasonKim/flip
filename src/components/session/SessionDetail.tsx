import { useCallback, useEffect, useRef, useState } from "react";
import type { SessionMeta } from "../../types/profile";
import { useSessionTimeline } from "../../hooks/useSessions";
import { resumeSession, revealSessionSource } from "../../lib/invoke";
import SessionPrompts from "./SessionPrompts";
import VirtualTimeline from "./VirtualTimeline";

interface SessionDetailProps {
  session: SessionMeta;
  debugEntriesVisible: boolean;
}

interface SessionSourceRevealNotice {
  kind: "success" | "error";
  message: string;
}

export default function SessionDetail({
  session,
  debugEntriesVisible,
}: SessionDetailProps) {
  const { timeline, entries, loading } = useSessionTimeline(
    session.agent,
    session.source_path,
  );

  const scrollRef = useRef<HTMLDivElement>(null);
  const copiedTimerRef = useRef<number | null>(null);
  const revealNoticeTimerRef = useRef<number | null>(null);
  const sourceRevealRequestRef = useRef(0);
  // tail -f 模式：默认跟随底部，用户上滚时脱离
  const [following, setFollowing] = useState(true);
  const [atTop, setAtTop] = useState(true);
  const [sessionIdCopied, setSessionIdCopied] = useState(false);
  const [sourceRevealPending, setSourceRevealPending] = useState(false);
  const [sourceRevealNotice, setSourceRevealNotice] =
    useState<SessionSourceRevealNotice | null>(null);
  // 用于检测用户主动滚动 vs 代码触发的滚动
  const programmaticScrollRef = useRef(false);
  const prevSnapshotRef = useRef("");

  const markProgrammaticScroll = useCallback(() => {
    programmaticScrollRef.current = true;
  }, []);

  const scrollToBottom = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    markProgrammaticScroll();
    el.scrollTop = el.scrollHeight;
  }, [markProgrammaticScroll]);

  const scrollToTop = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    markProgrammaticScroll();
    el.scrollTop = 0;
  }, [markProgrammaticScroll]);

  // 内容变化时，如果处于跟随模式则自动滚到底部
  useEffect(() => {
    // 用条目数量 + 最后一条内容长度作为快照，检测实际变化
    const snapshot = `${entries.length}:${entries[entries.length - 1]?.content.length ?? 0}`;
    if (snapshot !== prevSnapshotRef.current) {
      prevSnapshotRef.current = snapshot;
      if (following) {
        // 等 DOM 更新后再滚动
        requestAnimationFrame(scrollToBottom);
      }
    }
  }, [entries, following, scrollToBottom]);

  // 首次加载完成后直接跳到底部，并同步滚动状态
  useEffect(() => {
    if (!loading && entries.length > 0) {
      requestAnimationFrame(() => {
        scrollToBottom();
        setAtTop(false);
      });
    }
  }, [loading]); // eslint-disable-line react-hooks/exhaustive-deps

  // 用户主动上滚 → 脱离跟随；滚到底部 → 恢复跟随
  const handleScroll = useCallback(() => {
    if (programmaticScrollRef.current) {
      programmaticScrollRef.current = false;
      return;
    }
    const el = scrollRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 30;
    setFollowing(atBottom);
    setAtTop(el.scrollTop < 30);
  }, []);

  const inspectTimelineEntry = useCallback(() => {
    setFollowing(false);
  }, []);

  const copySessionId = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(session.session_id);
      setSessionIdCopied(true);
      if (copiedTimerRef.current !== null) {
        window.clearTimeout(copiedTimerRef.current);
      }
      copiedTimerRef.current = window.setTimeout(() => {
        setSessionIdCopied(false);
        copiedTimerRef.current = null;
      }, 1200);
    } catch (error) {
      console.warn(
        `[sessions] copy session id failed agent=${session.agent} session=${session.session_id}`,
        error,
      );
    }
  }, [session.agent, session.session_id]);

  const locateSessionSource = useCallback(async () => {
    if (!session.source_file_path) return;

    const requestId = sourceRevealRequestRef.current + 1;
    sourceRevealRequestRef.current = requestId;
    setSourceRevealPending(true);
    setSourceRevealNotice(null);
    try {
      await revealSessionSource(session.agent, session.source_file_path);
      if (sourceRevealRequestRef.current !== requestId) return;
      setSourceRevealNotice({ kind: "success", message: "已在文件管理器中定位" });
      if (revealNoticeTimerRef.current !== null) {
        window.clearTimeout(revealNoticeTimerRef.current);
      }
      revealNoticeTimerRef.current = window.setTimeout(() => {
        setSourceRevealNotice(null);
        revealNoticeTimerRef.current = null;
      }, 1800);
    } catch (error) {
      console.warn(
        `[sessions] source file reveal failed agent=${session.agent} session=${session.session_id} source=${session.source_file_path}`,
        error,
      );
      if (sourceRevealRequestRef.current === requestId) {
        setSourceRevealNotice({
          kind: "error",
          message: `定位失败：${String(error)}`,
        });
      }
    } finally {
      if (sourceRevealRequestRef.current === requestId) {
        setSourceRevealPending(false);
      }
    }
  }, [session.agent, session.session_id, session.source_file_path]);

  useEffect(() => {
    sourceRevealRequestRef.current += 1;
    setSessionIdCopied(false);
    setSourceRevealPending(false);
    setSourceRevealNotice(null);
    return () => {
      if (copiedTimerRef.current !== null) {
        window.clearTimeout(copiedTimerRef.current);
        copiedTimerRef.current = null;
      }
      if (revealNoticeTimerRef.current !== null) {
        window.clearTimeout(revealNoticeTimerRef.current);
        revealNoticeTimerRef.current = null;
      }
    };
  }, [session.session_id, session.source_file_path]);

  return (
    <div className="flex flex-col h-full relative">
      {/* 头部信息 */}
      <div className="shrink-0 border-b border-white/10 pb-3">
        <div className="flex items-center gap-2 min-w-0">
          <h2 className="text-base font-semibold text-gray-100 truncate flex-1 min-w-0">
            {session.title}
          </h2>
          <SessionPrompts session={session} />
          <button
            onClick={copySessionId}
            className="shrink min-w-0 max-w-[140px] sm:max-w-[260px] px-2 py-1 text-[11px] font-mono bg-white/5 hover:bg-white/15 text-gray-400 hover:text-gray-200 rounded-md transition-colors truncate"
            title={`点击复制会话 ID: ${session.session_id}`}
          >
            {sessionIdCopied ? "已复制" : `ID ${session.session_id}`}
          </button>
          {session.resume_command && (
            <button
              onClick={async () => {
                try {
                  await resumeSession(
                    session.resume_command!,
                    session.project_dir ?? undefined,
                  );
                } catch (error) {
                  console.warn(
                    `[sessions] resume failed agent=${session.agent} session=${session.session_id}; copied command to clipboard`,
                    error,
                  );
                  // fallback：复制命令到剪贴板
                  await navigator.clipboard.writeText(
                    session.resume_command!,
                  );
                }
              }}
              className="shrink-0 px-3 py-1 text-[11px] bg-white/10 hover:bg-white/20 text-gray-300 rounded-md transition-colors"
              title={session.resume_command}
            >
              ▶ 恢复
            </button>
          )}
          {session.source_file_path && (
            <button
              type="button"
              onClick={() => void locateSessionSource()}
              disabled={sourceRevealPending}
              className="shrink-0 rounded-md bg-white/10 px-3 py-1 text-[11px] text-gray-300 transition-colors hover:bg-white/20 disabled:cursor-wait disabled:opacity-60"
              title={session.source_file_path}
            >
              {sourceRevealPending ? "定位中…" : "定位会话文件"}
            </button>
          )}
        </div>
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 mt-1 text-[11px] text-gray-500">
          <span
            className={`uppercase font-bold ${
              session.agent === "claude"
                ? "text-violet-400"
                : session.agent === "codex"
                  ? "text-emerald-400"
                  : session.agent === "opencode"
                    ? "text-amber-400"
                    : "text-gray-400"
            }`}
          >
            {session.agent}
          </span>
          {session.source && <span className="rounded bg-sky-400/10 px-1.5 py-0.5 text-sky-300">{session.source.label}</span>}
          {session.project_dir && <span className="min-w-0 truncate" title={session.project_dir}>{session.project_dir}</span>}
          {session.last_active_at && (
            <span>{new Date(session.last_active_at).toLocaleString()}</span>
          )}
          {sourceRevealNotice && (
            <span
              className={
                sourceRevealNotice.kind === "error"
                  ? "min-w-0 truncate text-red-400"
                  : "text-emerald-400"
              }
              title={sourceRevealNotice.message}
            >
              {sourceRevealNotice.message}
            </span>
          )}
        </div>
      </div>

      <div
        ref={scrollRef}
        onScroll={handleScroll}
        className="flex-1 overflow-y-auto pr-1"
      >
        {loading ? (
          <p className="text-sm text-gray-500 animate-pulse">
            Loading timeline...
          </p>
        ) : entries.length === 0 ? (
          <p className="text-sm text-gray-500 italic">No records found</p>
        ) : (
          <VirtualTimeline
            timeline={timeline}
            agent={session.agent}
            sourcePath={session.source_path}
            projectDir={session.project_dir}
            scrollRef={scrollRef}
            debugEntriesVisible={debugEntriesVisible}
            followTail={following}
            onProgrammaticScroll={markProgrammaticScroll}
            onInspectEntry={inspectTimelineEntry}
          />
        )}
      </div>

      {(!atTop || !following) && (
        <div className="absolute bottom-4 right-6 z-40 flex flex-col gap-2">
          {!atTop && (
            <button
              onClick={() => {
                setAtTop(true);
                setFollowing(false);
                scrollToTop();
              }}
              className="flex h-8 w-8 items-center justify-center rounded-full bg-white/15 text-gray-300 shadow-lg backdrop-blur transition-colors hover:bg-white/25"
              title="Scroll to top"
            >
              ↑
            </button>
          )}
          {!following && (
            <button
              onClick={() => {
                setFollowing(true);
                setAtTop(false);
                scrollToBottom();
              }}
              className="flex h-8 w-8 items-center justify-center rounded-full bg-white/15 text-gray-300 shadow-lg backdrop-blur transition-colors hover:bg-white/25"
              title="Follow tail"
            >
              ↓
            </button>
          )}
        </div>
      )}
    </div>
  );
}
