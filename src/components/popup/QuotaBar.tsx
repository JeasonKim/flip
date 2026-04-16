import { useEffect, useState } from "react";
import type { QuotaTier } from "../../types/profile";

// 用量百分比 → 渐变色
function barGradient(pct: number): string {
  if (pct < 50) return "from-teal-400 to-cyan-400";
  if (pct < 80) return "from-amber-400 to-yellow-400";
  return "from-red-500 to-orange-500";
}

// 友好的 tier 名称
function tierDisplayName(name: string): string {
  const map: Record<string, string> = {
    five_hour: "5h",
    seven_day: "7d",
    seven_day_opus: "Opus",
    seven_day_sonnet: "Sonnet",
  };
  return map[name] ?? name;
}

// 倒计时格式：有天→精度到小时，有小时→精度到分钟，只有分钟→精度到秒
function formatCountdown(resetsAt: string | null): string {
  if (!resetsAt) return "";
  const diff = new Date(resetsAt).getTime() - Date.now();
  if (diff <= 0) return "";

  const d = Math.floor(diff / 86_400_000);
  const h = Math.floor((diff % 86_400_000) / 3_600_000);
  const m = Math.floor((diff % 3_600_000) / 60_000);
  const s = Math.floor((diff % 60_000) / 1_000);

  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${String(m).padStart(2, "0")}m`;
  return `${m}m ${String(s).padStart(2, "0")}s`;
}

interface QuotaBarProps {
  tier: QuotaTier;
  onExpired?: () => void;
}

export default function QuotaBar({ tier, onExpired }: QuotaBarProps) {
  const [countdown, setCountdown] = useState(() =>
    formatCountdown(tier.resets_at),
  );

  useEffect(() => {
    if (!tier.resets_at) return;

    const tick = () => {
      const text = formatCountdown(tier.resets_at);
      setCountdown(text);
      // 倒计时归零：触发刷新，停止计时
      if (!text) {
        onExpired?.();
        clearInterval(id);
      }
    };

    tick();
    const id = setInterval(tick, 1_000);
    return () => clearInterval(id);
  }, [tier.resets_at, onExpired]);

  const pct = Math.min(100, Math.max(0, tier.utilization));

  return (
    <div className="flex items-center gap-1.5 text-xs whitespace-nowrap">
      <span className="w-11 shrink-0 text-gray-400 font-medium">
        {tierDisplayName(tier.name)}
      </span>
      <div className="flex-1 min-w-0 h-2 rounded-full bg-white/10 overflow-hidden">
        <div
          className={`h-full rounded-full bg-gradient-to-r ${barGradient(pct)} transition-all duration-500`}
          style={{ width: `${pct}%` }}
        />
      </div>
      <span className="w-8 shrink-0 text-right tabular-nums text-gray-300">
        {pct.toFixed(0)}%
      </span>
      {countdown && (
        <span className="shrink-0 tabular-nums text-gray-500 font-mono text-[10px]">
          {countdown}
        </span>
      )}
    </div>
  );
}
