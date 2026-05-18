import { useCallback, useEffect, useRef, useState } from "react";
import type { SessionMeta } from "../../types/profile";
import { useSessionMessages } from "../../hooks/useSessions";
import { resumeSession } from "../../lib/invoke";
import MessageBubble from "./MessageBubble";

interface SessionDetailProps {
  session: SessionMeta;
}

export default function SessionDetail({ session }: SessionDetailProps) {
  const { messages, loading } = useSessionMessages(
    session.agent,
    session.source_path,
  );

  const scrollRef = useRef<HTMLDivElement>(null);
  const copiedTimerRef = useRef<number | null>(null);
  // tail -f 模式：默认跟随底部，用户上滚时脱离
  const [following, setFollowing] = useState(true);
  const [atTop, setAtTop] = useState(true);
  const [sessionIdCopied, setSessionIdCopied] = useState(false);
  // 用于检测用户主动滚动 vs 代码触发的滚动
  const programmaticScrollRef = useRef(false);
  const prevSnapshotRef = useRef("");

  const scrollToBottom = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    programmaticScrollRef.current = true;
    el.scrollTop = el.scrollHeight;
  }, []);

  const scrollToTop = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    programmaticScrollRef.current = true;
    el.scrollTop = 0;
  }, []);

  // 内容变化时，如果处于跟随模式则自动滚到底部
  useEffect(() => {
    // 用消息数量 + 最后一条内容长度作为快照，检测实际变化
    const snapshot = `${messages.length}:${messages[messages.length - 1]?.content.length ?? 0}`;
    if (snapshot !== prevSnapshotRef.current) {
      prevSnapshotRef.current = snapshot;
      if (following) {
        // 等 DOM 更新后再滚动
        requestAnimationFrame(scrollToBottom);
      }
    }
  }, [messages, following, scrollToBottom]);

  // 首次加载完成后直接跳到底部，并同步滚动状态
  useEffect(() => {
    if (!loading && messages.length > 0) {
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

  useEffect(() => {
    setSessionIdCopied(false);
    return () => {
      if (copiedTimerRef.current !== null) {
        window.clearTimeout(copiedTimerRef.current);
        copiedTimerRef.current = null;
      }
    };
  }, [session.session_id]);

  return (
    <div className="flex flex-col h-full relative">
      {/* 头部信息 */}
      <div className="shrink-0 border-b border-white/10 pb-3 mb-3">
        <div className="flex items-center gap-2 min-w-0">
          <h2 className="text-base font-semibold text-gray-100 truncate flex-1 min-w-0">
            {session.title}
          </h2>
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
        </div>
        <div className="flex items-center gap-3 mt-1 text-[11px] text-gray-500">
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
          {session.project_dir && <span>{session.project_dir}</span>}
          {session.last_active_at && (
            <span>{new Date(session.last_active_at).toLocaleString()}</span>
          )}
        </div>
      </div>

      {/* 消息列表 */}
      <div
        ref={scrollRef}
        onScroll={handleScroll}
        className="flex-1 overflow-y-auto space-y-2 pr-1"
      >
        {loading ? (
          <p className="text-sm text-gray-500 animate-pulse">
            Loading messages...
          </p>
        ) : messages.length === 0 ? (
          <p className="text-sm text-gray-500 italic">No messages found</p>
        ) : (
          messages.map((msg, i) => <MessageBubble key={i} message={msg} />)
        )}
      </div>

      {/* 右上角：直达底部 */}
      {!following && (
        <button
          onClick={() => {
            setFollowing(true);
            setAtTop(false);
            scrollToBottom();
          }}
          className="absolute top-16 right-6 bg-white/15 hover:bg-white/25 text-gray-300 rounded-full w-8 h-8 flex items-center justify-center shadow-lg backdrop-blur transition-colors"
          title="Follow tail"
        >
          ↓
        </button>
      )}

      {/* 右下角：直达顶部 */}
      {!atTop && (
        <button
          onClick={() => {
            setAtTop(true);
            setFollowing(false);
            scrollToTop();
          }}
          className="absolute bottom-4 right-6 bg-white/15 hover:bg-white/25 text-gray-300 rounded-full w-8 h-8 flex items-center justify-center shadow-lg backdrop-blur transition-colors"
          title="Scroll to top"
        >
          ↑
        </button>
      )}
    </div>
  );
}
