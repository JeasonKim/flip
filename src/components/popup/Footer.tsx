import * as api from "../../lib/invoke";

export default function Footer() {
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
          账号配置
        </button>
      </div>
      <span className="text-[10px] text-gray-600">Flip</span>
    </div>
  );
}
