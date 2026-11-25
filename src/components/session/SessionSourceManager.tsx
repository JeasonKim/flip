import { useEffect, useRef, useState } from "react";
import type { SessionSource, SessionSourceConfig } from "../../types/profile";
import { configureSessionSources } from "../../lib/invoke";

interface SessionSourceManagerProps {
  sources: SessionSource[];
  onSaved: (sources: SessionSource[]) => void;
  onClose: () => void;
}

export default function SessionSourceManager({ sources, onSaved, onClose }: SessionSourceManagerProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [drafts, setDrafts] = useState<SessionSourceConfig[]>(() => sources
    .filter((source) => !source.is_default)
    .map(({ id, label, path, enabled }) => ({ id, label, path, enabled })));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const native = sources.find((source) => source.is_default);

  useEffect(() => { dialogRef.current?.showModal(); }, []);

  function reviseSource(id: string, change: Partial<SessionSourceConfig>) {
    setDrafts((current) => current.map((source) => source.id === id ? { ...source, ...change } : source));
  }

  async function applySources(event: React.FormEvent) {
    event.preventDefault();
    if (saving) return;
    setSaving(true);
    setError(null);
    try {
      const next = await configureSessionSources(drafts);
      onSaved(next);
      onClose();
    } catch (cause) {
      console.warn("[session-sources] configuration failed", cause);
      setError(String(cause));
    } finally {
      setSaving(false);
    }
  }

  return (
    <dialog ref={dialogRef} onCancel={(event) => { if (saving) event.preventDefault(); else onClose(); }}
      aria-labelledby="source-manager-title"
      className="m-auto w-[620px] max-w-[calc(100vw-32px)] max-h-[calc(100vh-40px)] rounded-xl border border-white/15 bg-gray-900 p-0 text-gray-100 shadow-2xl backdrop:bg-black/60">
      <form onSubmit={applySources} className="flex max-h-[calc(100vh-40px)] flex-col">
        <div className="border-b border-white/10 px-5 py-4">
          <h2 id="source-manager-title" className="text-base font-semibold">采集目录</h2>
          <p className="mt-1 text-xs leading-5 text-gray-400">为每个 Codex 数据目录设置来源标签，在会话列表中快速区分和筛选。</p>
        </div>
        <div className="min-h-0 space-y-3 overflow-y-auto px-5 py-4">
          {native && (
            <div className="rounded-lg border border-white/10 bg-white/[0.03] p-3">
              <div className="flex items-center gap-2 text-sm"><span>{native.label}</span><span className="text-[10px] text-gray-500">默认 · 始终采集</span></div>
              <p className="mt-1 break-all text-xs text-gray-400">{native.path}</p>
              {native.issue && <p className="mt-2 text-xs text-amber-400">{native.issue}</p>}
            </div>
          )}
          {drafts.map((source) => {
            const issue = sources.find((original) => original.id === source.id && original.path === source.path)?.issue;
            return (
              <fieldset key={source.id} disabled={saving} className="rounded-lg border border-white/10 p-3">
                <div className="flex items-end gap-3">
                  <label className="min-w-0 flex-1 text-xs text-gray-400">来源标签
                    <input required maxLength={40} value={source.label} onChange={(event) => reviseSource(source.id, { label: event.target.value })}
                      placeholder="例如：SkillFlow Dev" className="mt-1 block w-full rounded-md border border-white/15 bg-gray-950 px-3 py-2 text-sm text-gray-100 outline-none focus:border-emerald-500" />
                  </label>
                  <label className="flex shrink-0 items-center gap-1.5 pb-2 text-xs text-gray-400">
                    <input type="checkbox" checked={source.enabled} onChange={(event) => reviseSource(source.id, { enabled: event.target.checked })} className="accent-emerald-500" />启用
                  </label>
                  <button type="button" onClick={() => setDrafts((current) => current.filter((item) => item.id !== source.id))}
                    aria-label={`移除来源 ${source.label || "未命名"}`} className="pb-2 text-xs text-gray-500 hover:text-red-400">移除</button>
                </div>
                <label className="mt-3 block text-xs text-gray-400">Codex 数据根目录
                  <input required value={source.path} onChange={(event) => reviseSource(source.id, { path: event.target.value })}
                    placeholder="/Users/you/project/codex" spellCheck={false} className="mt-1 block w-full rounded-md border border-white/15 bg-gray-950 px-3 py-2 font-mono text-xs text-gray-100 outline-none focus:border-emerald-500" />
                </label>
                {issue && <p className="mt-2 text-xs text-amber-400">{issue}</p>}
              </fieldset>
            );
          })}
          <button type="button" disabled={saving} onClick={() => setDrafts((current) => [...current, { id: `custom-${crypto.randomUUID()}`, label: "", path: "", enabled: true }])}
            className="w-full rounded-lg border border-dashed border-white/20 py-2.5 text-xs text-gray-400 hover:border-emerald-500/50 hover:text-emerald-300 disabled:opacity-50">＋ 添加目录</button>
          <p className="text-[11px] leading-5 text-gray-500">自动采集目录内的 sessions 和 archived_sessions。自定义目录只读；停用或移除来源会保留原始文件。</p>
          {error && <p role="alert" className="break-words text-xs text-red-400">{error}</p>}
        </div>
        <div className="flex justify-end gap-2 border-t border-white/10 px-5 py-3">
          <button type="button" disabled={saving} onClick={onClose} className="rounded-md px-3 py-2 text-xs text-gray-400 hover:bg-white/5 disabled:opacity-50">取消</button>
          <button type="submit" disabled={saving} className="rounded-md bg-emerald-500/15 px-4 py-2 text-xs text-emerald-300 hover:bg-emerald-500/25 disabled:opacity-50">{saving ? "保存中…" : "保存并采集"}</button>
        </div>
      </form>
    </dialog>
  );
}
