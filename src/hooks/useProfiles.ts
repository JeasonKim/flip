import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
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
      api
        .reconcileLiveProfiles()
        .then((liveConfig) => {
          setConfig(liveConfig);
          setError(null);
        })
        .catch((e) => {
          setError(String(e));
        });
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    reload();
  }, [reload]);

  useEffect(() => {
    window.addEventListener("focus", reload);
    return () => window.removeEventListener("focus", reload);
  }, [reload]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    listen("flip-popup-shown", reload)
      .then((release) => {
        if (disposed) {
          release();
          return;
        }
        unlisten = release;
      })
      .catch((error) => {
        console.warn("[profiles] failed to listen popup shown event", error);
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
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
