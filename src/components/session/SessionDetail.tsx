import { useCallback, useEffect, useRef, useState } from "react";
import type { SessionMeta } from "../../types/profile";
import { useSessionMessages } from "../../hooks/useSessions";
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
  // tail -f 模式：默认跟随底部，用户上滚时脱离
  const [following, setFollowing] = useState(true);
  const [atTop, setAtTop] = useState(true);
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

  return (
    <div className="flex flex-col h-full relative">
      {/* 头部信息 */}
      <div className="shrink-0 border-b border-white/10 pb-3 mb-3">
        <h2 className="text-base font-semibold text-gray-100 truncate">
          {session.title}
        </h2>
        <div className="flex items-center gap-3 mt-1 text-[11px] text-gray-500">
          <span
            className={`uppercase font-bold ${
              session.agent === "claude"
                ? "text-violet-400"
                : "text-emerald-400"
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
