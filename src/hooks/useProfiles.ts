import { useCallback, useEffect, useState } from "react";
import type { AgentId, FlipConfig } from "../types/profile";
import * as api from "../lib/invoke";

export function useProfiles() {
  const [config, setConfig] = useState<FlipConfig | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      const data = await api.listProfiles();
      setConfig(data);
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    reload();
  }, [reload]);

  const flip = useCallback(
    async (agent: AgentId, accountId: string) => {
      await api.flipAccount(agent, accountId);
      await reload();
    },
    [reload],
  );

  const capture = useCallback(
    async (agent: AgentId) => {
      const account = await api.captureCurrentAccount(agent);
      await reload();
      return account;
    },
    [reload],
  );

  const dismiss = useCallback(
    async (agent: AgentId, accountId: string) => {
      await api.dismissAccount(agent, accountId);
      await reload();
    },
    [reload],
  );

  const rename = useCallback(
    async (agent: AgentId, accountId: string, newLabel: string) => {
      await api.renameAccount(agent, accountId, newLabel);
      await reload();
    },
    [reload],
  );

  return { config, loading, error, reload, flip, capture, dismiss, rename };
}
