import { useCallback, useState } from "react";
import * as api from "../../lib/invoke";

interface FooterProps {
  onImported?: () => void;
}

export default function Footer({ onImported }: FooterProps) {
  const [importMsg, setImportMsg] = useState<string | null>(null);

  const handleImport = useCallback(async () => {
    try {
      const result = await api.importFromCcswitch();
      setImportMsg(`导入 ${result.imported} 个，跳过 ${result.skipped} 个`);
      setTimeout(() => setImportMsg(null), 3000);
      onImported?.();
    } catch (e) {
      setImportMsg(`${e}`);
      setTimeout(() => setImportMsg(null), 3000);
    }
  }, [onImported]);

  return (
    <div className="flex items-center justify-between pt-2 border-t border-white/10">
      <div className="flex items-center gap-3">
        <button
          onClick={() => api.openSessionWindow()}
          className="text-xs text-gray-400 hover:text-white transition-colors"
        >
          会话记录
        </button>
        <button
          onClick={() => api.revealConfigFile()}
          className="text-xs text-gray-400 hover:text-white transition-colors"
        >
          配置文件
        </button>
        <button
          onClick={handleImport}
          className="text-xs text-gray-400 hover:text-white transition-colors"
          title="从 CC Switch 导入账号"
        >
          导入
        </button>
      </div>
      <div className="flex items-center gap-2">
        {importMsg && (
          <span className="text-[10px] text-emerald-400 animate-pulse">
            {importMsg}
          </span>
        )}
        <span className="text-[10px] text-gray-600">Flip</span>
      </div>
    </div>
  );
}
