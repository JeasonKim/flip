import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import type { SessionMessage } from "../../types/profile";

// 角色 → 颜色配置
const roleStyle: Record<string, { bg: string; label: string; labelColor: string }> = {
  user: {
    bg: "bg-blue-500/10 border-blue-500/20",
    label: "User",
    labelColor: "text-blue-400",
  },
  assistant: {
    bg: "bg-violet-500/10 border-violet-500/20",
    label: "Assistant",
    labelColor: "text-violet-400",
  },
  tool: {
    bg: "bg-gray-500/10 border-gray-500/20",
    label: "Tool",
    labelColor: "text-gray-400",
  },
  system: {
    bg: "bg-amber-500/10 border-amber-500/20",
    label: "System",
    labelColor: "text-amber-400",
  },
};

function formatTimestamp(ts: number | null): string {
  if (!ts) return "";
  const d = new Date(ts);
  return d.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

interface MessageBubbleProps {
  message: SessionMessage;
}

export default function MessageBubble({ message }: MessageBubbleProps) {
  const style = roleStyle[message.role] ?? roleStyle.system;
  const isToolOutput = message.role === "tool";

  return (
    <div className={`rounded-lg border p-3 ${style.bg}`}>
      {/* 头部：角色 + 时间 */}
      <div className="flex items-center justify-between mb-1.5">
        <span className={`text-xs font-semibold ${style.labelColor}`}>
          {style.label}
        </span>
        <span className="text-[10px] text-gray-500 tabular-nums">
          {formatTimestamp(message.timestamp)}
        </span>
      </div>

      {/* 消息内容 */}
      {isToolOutput ? (
        <pre className="text-xs text-gray-400 whitespace-pre-wrap break-all max-h-40 overflow-y-auto font-mono">
          {message.content}
        </pre>
      ) : (
        <div className="prose prose-invert prose-sm max-w-none break-words
          prose-pre:bg-black/30 prose-pre:text-xs prose-code:text-xs">
          <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
            {message.content}
          </ReactMarkdown>
        </div>
      )}
    </div>
  );
}
