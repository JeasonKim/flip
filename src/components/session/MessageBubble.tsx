import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import {
  Children,
  isValidElement,
  memo,
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import type { Components } from "react-markdown";
import type { SessionMessage } from "../../types/profile";
import {
  isMermaidDiagramSource,
  normalizeMarkdownDiagrams,
} from "../../lib/markdownDiagrams";
import MermaidBlock from "./MermaidBlock";

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

interface CodeElementProps {
  className?: string;
  children?: ReactNode;
}

interface MarkdownCodeBlock {
  code: string;
  language: string | null;
}

const markdownComponents: Components = {
  pre({ children, ...props }) {
    const codeBlock = extractMarkdownCodeBlock(children);
    if (codeBlock && shouldRenderMermaidDiagram(codeBlock)) {
      return <MermaidBlock chart={codeBlock.code} />;
    }

    return <pre {...props}>{children}</pre>;
  },
};

function MessageBubble({ message }: MessageBubbleProps) {
  const style = roleStyle[message.role] ?? roleStyle.system;
  const isToolOutput = message.role === "tool";
  const markdownContent = normalizeMarkdownDiagrams(message.content);

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
        <ToolOutputBlock content={message.content} />
      ) : (
        <div className="prose prose-invert prose-sm max-w-none break-words
          prose-pre:bg-black/30 prose-pre:text-xs prose-code:text-xs">
          <ReactMarkdown
            remarkPlugins={[remarkGfm]}
            rehypePlugins={[rehypeHighlight]}
            components={markdownComponents}
          >
            {markdownContent}
          </ReactMarkdown>
        </div>
      )}
    </div>
  );
}

export default memo(MessageBubble);

interface ToolOutputBlockProps {
  content: string;
}

function ToolOutputBlock({ content }: ToolOutputBlockProps) {
  const toolOutputRef = useRef<HTMLPreElement>(null);
  const [scrollActive, setScrollActive] = useState(false);

  useEffect(() => {
    setScrollActive(false);
  }, [content]);

  const activateToolScroll = useCallback(() => {
    const toolOutput = toolOutputRef.current;
    if (!toolOutput) {
      return;
    }

    setScrollActive(toolOutput.scrollHeight > toolOutput.clientHeight + 1);
  }, []);

  const deactivateToolScroll = useCallback(() => {
    setScrollActive(false);
  }, []);

  return (
    <pre
      ref={toolOutputRef}
      onClick={activateToolScroll}
      onMouseLeave={deactivateToolScroll}
      className={`max-h-40 whitespace-pre-wrap break-all font-mono text-xs text-gray-400 ${
        scrollActive ? "overflow-y-auto" : "overflow-y-hidden"
      }`}
      style={{ overscrollBehavior: scrollActive ? "contain" : "auto" }}
      title={scrollActive ? "Tool output scroll active" : "Click to scroll tool output"}
    >
      {content}
    </pre>
  );
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
