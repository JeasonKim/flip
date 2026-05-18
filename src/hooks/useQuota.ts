import { useCallback, useEffect, useRef, useState } from "react";
import type { AgentId, QuotaTier } from "../types/profile";
import * as api from "../lib/invoke";

export function useQuota(agent: AgentId, enabled: boolean) {
  const [tiers, setTiers] = useState<QuotaTier[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const fetch = useCallback(async () => {
    if (!enabled) return;
    setLoading(true);
    try {
      const result = await api.fetchQuota(agent);
      if (result.success) {
        setTiers(result.tiers);
        setError(null);
      } else {
        console.warn(
          `[quota] fetch failed agent=${agent}: ${result.error ?? "unknown error"}`,
        );
        setError(result.error ?? "unknown error");
      }
    } catch (e) {
      console.warn(`[quota] fetch threw agent=${agent}`, e);
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [agent, enabled]);

  useEffect(() => {
    fetch();
    // 每 60 秒刷新一次
    timerRef.current = setInterval(fetch, 60_000);
    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
    };
  }, [fetch]);

  return { tiers, loading, error, refresh: fetch };
}
