import { useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { loadSessionSystemPrompts } from "../../lib/invoke";
import {
  defaultPromptScope,
  isCustomSessionPrompt,
  prioritizeSessionPrompts,
  visibleSessionPrompts,
  type SessionPromptScope,
} from "../../lib/sessionPrompts";
import type { SessionMeta, SessionPrompt } from "../../types/profile";

const originLabels: Record<SessionPrompt["origin"], { tab: string; detail: string }> = {
  configured: { tab: "自定义", detail: "显式配置" },
  request: { tab: "传入", detail: "请求传入" },
  model: { tab: "模型", detail: "模型模板" },
  runtime: { tab: "运行时", detail: "运行时注入" },
  snapshot: { tab: "快照", detail: "提示词快照" },
  unknown: { tab: "", detail: "来源未标记" },
};

const emptyPrompts: SessionPrompt[] = [];

type PromptLoadState =
  | { status: "loading" }
  | { status: "failed" }
  | { status: "ready"; prompts: SessionPrompt[] };

interface PromptCopyFeedback {
  prompt: SessionPrompt;
  message: string;
}

interface SessionPromptsProps {
  session: SessionMeta;
}

const actionClass =
  "shrink-0 rounded-md bg-white/10 px-3 py-1.5 text-xs text-gray-200 transition-colors hover:bg-white/20";

export default function SessionPrompts({ session }: SessionPromptsProps) {
  const { agent, source_path: sourcePath } = session;
  const viewId = useId();
  const panelId = `${viewId}-panel`;
  const dialogRef = useRef<HTMLDialogElement>(null);
  const tabListRef = useRef<HTMLDivElement>(null);
  const loadVersion = useRef(0);
  const [load, setLoad] = useState<PromptLoadState | null>(null);
  const [scope, setScope] = useState<SessionPromptScope>("all");
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [copyFeedback, setCopyFeedback] = useState<PromptCopyFeedback | null>(null);
  const allPrompts = load?.status === "ready" ? load.prompts : emptyPrompts;
  const prompts = useMemo(
    () => visibleSessionPrompts(allPrompts, scope, query),
    [allPrompts, scope, query],
  );
  const selected = prompts[selectedIndex];
  const hasTabs = prompts.length > 1;
  const showScopeFilter = allPrompts.some(isCustomSessionPrompt) &&
    allPrompts.some((prompt) => !isCustomSessionPrompt(prompt));

  async function inspectPrompts() {
    const version = ++loadVersion.current;
    setLoad({ status: "loading" });
    setQuery("");
    resetSelection();
    try {
      const prompts = await loadSessionSystemPrompts(agent, sourcePath);
      if (version === loadVersion.current) {
        setScope(defaultPromptScope(prompts));
        setLoad({ status: "ready", prompts: prioritizeSessionPrompts(prompts) });
      }
    } catch (error) {
      console.warn(
        `[sessions] system prompts load failed agent=${agent} source=${sourcePath}`,
        error,
      );
      if (version === loadVersion.current) setLoad({ status: "failed" });
    }
  }

  function resetSelection() {
    setSelectedIndex(0);
    setCopyFeedback(null);
    if (tabListRef.current) tabListRef.current.scrollLeft = 0;
  }

  function discardPromptView() {
    if (dialogRef.current?.open) return;
    loadVersion.current += 1;
    setLoad(null);
    setCopyFeedback(null);
  }

  function selectPrompt(index: number, tab: HTMLButtonElement) {
    if (index !== selectedIndex) {
      setSelectedIndex(index);
      setCopyFeedback(null);
    }
    tab.focus({ preventScroll: true });
    tab.scrollIntoView({ block: "nearest", inline: "nearest" });
  }

  function navigatePromptTabs(event: KeyboardEvent<HTMLDivElement>) {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;

    let index: number;
    switch (event.key) {
      case "ArrowLeft":
        index = (selectedIndex + prompts.length - 1) % prompts.length;
        break;
      case "ArrowRight":
        index = (selectedIndex + 1) % prompts.length;
        break;
      case "Home":
        index = 0;
        break;
      case "End":
        index = prompts.length - 1;
        break;
      default:
        return;
    }

    const tab = event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="tab"]')[index];
    if (tab) {
      event.preventDefault();
      selectPrompt(index, tab);
    }
  }

  async function copyPrompt(prompt: SessionPrompt) {
    const version = loadVersion.current;
    try {
      await navigator.clipboard.writeText(prompt.text);
      if (version === loadVersion.current) setCopyFeedback({ prompt, message: "已复制" });
    } catch (error) {
      console.warn(`[sessions] system prompt copy failed agent=${agent}`, error);
      if (version === loadVersion.current) setCopyFeedback({ prompt, message: "复制失败" });
    }
  }

  return (
    <>
      <button
        type="button"
        aria-haspopup="dialog"
        onClick={() => {
          dialogRef.current?.showModal();
          void inspectPrompts();
        }}
        className="shrink-0 rounded-md bg-white/10 px-3 py-1 text-[11px] text-gray-300 transition-colors hover:bg-white/20"
      >
        系统提示词
      </button>
      <dialog
        ref={dialogRef}
        onClose={discardPromptView}
        aria-labelledby="session-prompt-title"
        className="m-auto w-[880px] max-w-[calc(100vw-32px)] rounded-xl border border-white/15 bg-gray-900 p-0 text-gray-100 shadow-2xl backdrop:bg-black/60"
      >
        <div className="flex max-h-[calc(100vh-40px)] flex-col">
          <div className="flex shrink-0 items-center gap-3 border-b border-white/10 px-5 py-3">
            <h2 id="session-prompt-title" className="flex-1 text-base font-semibold">
              系统提示词
            </h2>
            <button
              type="button"
              onClick={() => dialogRef.current?.close()}
              className="shrink-0 rounded-md px-3 py-1.5 text-xs text-gray-400 hover:bg-white/5 hover:text-gray-200"
            >
              关闭
            </button>
          </div>
          {load?.status === "loading" && (
            <p role="status" className="px-5 py-5 text-sm text-gray-400">
              正在读取会话记录…
            </p>
          )}
          {load?.status === "failed" && (
            <div className="flex items-center justify-between gap-3 px-5 py-4">
              <p role="alert" className="text-sm text-red-300">读取系统提示词失败。</p>
              <button
                type="button"
                className={actionClass}
                onClick={() => void inspectPrompts()}
              >
                重试
              </button>
            </div>
          )}
          {load?.status === "ready" && allPrompts.length === 0 && (
            <p className="px-5 py-5 text-sm text-gray-400">
              该会话的原始记录中没有可读取的系统提示词。
            </p>
          )}
          {allPrompts.length > 1 && (
            <div className="flex shrink-0 items-center gap-3 px-5 pt-3 pb-1">
              {showScopeFilter && (
                <div role="group" aria-label="提示词来源筛选" className="flex gap-1 rounded-md bg-white/5 p-0.5">
                  {(["custom", "all"] as const).map((option) => (
                    <button
                      key={option}
                      type="button"
                      aria-pressed={scope === option}
                      onClick={() => {
                        if (option === scope) return;
                        setScope(option);
                        resetSelection();
                      }}
                      className="rounded px-2 py-1 text-xs text-gray-400 hover:text-gray-100 aria-pressed:bg-white/15 aria-pressed:text-gray-100"
                    >
                      {option === "custom" ? "自定义" : "全部"}
                    </button>
                  ))}
                </div>
              )}
              <input
                type="search"
                aria-label="搜索提示词原文"
                placeholder="搜索提示词原文"
                value={query}
                onChange={(event) => {
                  setQuery(event.target.value);
                  resetSelection();
                }}
                className="min-w-0 max-w-64 flex-1 rounded-md border border-white/10 bg-gray-950 px-2.5 py-1.5 text-xs text-gray-200 placeholder:text-gray-500 focus:border-sky-500 focus:outline-none"
              />
            </div>
          )}
          {allPrompts.length > 0 && !selected && (
            <p role="status" className="px-5 py-5 text-sm text-gray-400">
              当前范围没有匹配的提示词。
            </p>
          )}
          {selected && (
            <>
              {hasTabs && (
                <div
                  ref={tabListRef}
                  role="tablist"
                  aria-label="选择提示词"
                  onKeyDown={navigatePromptTabs}
                  className="mx-5 mt-2 flex min-w-0 shrink-0 gap-1 overflow-x-auto overscroll-x-contain pb-1 [&::-webkit-scrollbar]:h-1.5"
                >
                  {prompts.map((prompt, index) => (
                    <button
                      key={index}
                      id={`${viewId}-tab-${index}`}
                      type="button"
                      role="tab"
                      aria-controls={panelId}
                      aria-selected={index === selectedIndex}
                      tabIndex={index === selectedIndex ? 0 : -1}
                      title={promptSourceDetail(prompt)}
                      onClick={(event) => selectPrompt(index, event.currentTarget)}
                      className="shrink-0 whitespace-nowrap rounded-md px-3 py-1.5 text-xs text-gray-400 hover:bg-white/10 hover:text-gray-200 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-sky-400 aria-selected:bg-sky-400/15 aria-selected:text-sky-200"
                    >
                      {promptTabLabel(prompt)}
                    </button>
                  ))}
                </div>
              )}
              <div className="flex shrink-0 items-center gap-3 px-5 py-2">
                <span className="min-w-0 break-words text-xs text-gray-400">{promptSourceDetail(selected)}</span>
                <div className="ml-auto flex shrink-0 items-center gap-2">
                  <span role="status" className="w-[4em] text-right text-xs text-gray-400">
                    {copyFeedback?.prompt === selected ? copyFeedback.message : ""}
                  </span>
                  <button type="button" className={actionClass} onClick={() => void copyPrompt(selected)}>
                    复制原文
                  </button>
                </div>
              </div>
              <pre
                key={`${selected.source.section}:${selected.source.index}:${selected.role}:${selected.origin}`}
                id={panelId}
                role={hasTabs ? "tabpanel" : undefined}
                tabIndex={0}
                aria-labelledby={hasTabs ? `${viewId}-tab-${selectedIndex}` : undefined}
                aria-label={hasTabs ? undefined : "提示词原文"}
                className="mx-5 mb-5 min-h-0 overflow-auto whitespace-pre-wrap rounded-lg border border-white/10 bg-gray-950 p-4 font-mono text-xs leading-relaxed wrap-anywhere"
              >
                {selected.text}
              </pre>
            </>
          )}
        </div>
      </dialog>
    </>
  );
}

function promptTabLabel(prompt: SessionPrompt): string {
  const role = prompt.role === "developer" ? "Dev" : "Sys";
  const label = prompt.origin === "snapshot" && prompt.entrypoint?.startsWith("sdk-")
    ? "SDK"
    : originLabels[prompt.origin].tab || role;
  const sourceIndex = prompt.source.section === "message"
    ? `#${prompt.source.index}`
    : `L${prompt.source.index}`;
  return `${label} ${sourceIndex}`;
}

function promptSourceDetail(prompt: SessionPrompt): string {
  const { source } = prompt;
  const role = prompt.role === "developer" ? "Developer" : "System";
  const location = source.section === "message"
    ? `消息 #${source.index}`
    : `日志第 ${source.index} 行`;
  return [role, originLabels[prompt.origin].detail, location, prompt.entrypoint].filter(Boolean).join(" · ");
}
