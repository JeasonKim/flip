import { useCallback, useEffect, useRef, useState } from "react";
import type {
  SessionAgentId,
  SessionMeta,
  SessionMessage,
  SessionSourceRevision,
  SessionRawContent,
} from "../types/profile";
import * as api from "../lib/invoke";
import {
  sessionListsShareVisibleState,
  sessionMessagesShareTranscript,
  sessionSourceRevisionChanged,
} from "../lib/sessionRefresh";

const PAGE_SIZE = 20;

const REFRESH_INTERVAL = 10000;

export function useSessions() {
  const [sessions, setSessions] = useState<SessionMeta[]>([]);
  const [loading, setLoading] = useState(true);
  const [hasMore, setHasMore] = useState(true);

  const [agentFilter, setAgentFilter] = useState<SessionAgentId | undefined>();

  // 当前已加载的总条数，用于静默刷新时保持范围一致
  const loadedCountRef = useRef(PAGE_SIZE);
  const refreshInFlightRef = useRef(false);

  const loadInitial = useCallback(async () => {
    setLoading(true);
    try {
      const data = await api.scanSessions(agentFilter, 0, PAGE_SIZE);
      setSessions((current) =>
        sessionListsShareVisibleState(current, data) ? current : data,
      );
      setHasMore(data.length >= PAGE_SIZE);
      loadedCountRef.current = data.length;
    } finally {
      setLoading(false);
    }
  }, [agentFilter]);

  const loadMore = useCallback(async () => {
    const data = await api.scanSessions(
      agentFilter,
      sessions.length,
      PAGE_SIZE,
    );
    setSessions((prev) => {
      const next = [...prev, ...data];
      return sessionListsShareVisibleState(prev, next) ? prev : next;
    });
    setHasMore(data.length >= PAGE_SIZE);
    loadedCountRef.current = sessions.length + data.length;
  }, [agentFilter, sessions.length]);

  useEffect(() => {
    loadInitial();
  }, [loadInitial]);

  // 定时静默刷新：不触发 loading，不影响选中状态
  useEffect(() => {
    const timer = setInterval(async () => {
      if (refreshInFlightRef.current) {
        return;
      }

      refreshInFlightRef.current = true;
      try {
        const count = Math.max(loadedCountRef.current, PAGE_SIZE);
        const data = await api.scanSessions(agentFilter, 0, count);
        setSessions((current) =>
          sessionListsShareVisibleState(current, data) ? current : data,
        );
        setHasMore(data.length >= count);
      } catch (error) {
        console.warn(
          `[sessions] refresh failed agent=${agentFilter ?? "all"} count=${Math.max(
            loadedCountRef.current,
            PAGE_SIZE,
          )}`,
          error,
        );
      } finally {
        refreshInFlightRef.current = false;
      }
    }, REFRESH_INTERVAL);
    return () => clearInterval(timer);
  }, [agentFilter]);

  return {
    sessions,
    loading,
    hasMore,
    agentFilter,
    setAgentFilter,
    loadMore,
    refresh: loadInitial,
  };
}

const MESSAGE_REFRESH_INTERVAL = 3000;

export function useSessionMessages(agent: string, sourcePath: string) {
  const [messages, setMessages] = useState<SessionMessage[]>([]);
  const [loading, setLoading] = useState(true);
  const sourceRevisionRef = useRef<SessionSourceRevision | null>(null);
  const refreshInFlightRef = useRef(false);

  const refreshMessages = useCallback(
    async (force: boolean) => {
      if (refreshInFlightRef.current) {
        return;
      }

      refreshInFlightRef.current = true;
      let nextRevision: SessionSourceRevision | null = null;
      try {
        nextRevision = await api.readSessionSourceRevision(agent, sourcePath);
      } catch (error) {
        console.warn(
          `[sessions] message source revision failed agent=${agent} source=${sourcePath}; falling back to full reload`,
          error,
        );
      }

      try {
        if (
          !force &&
          !sessionSourceRevisionChanged(sourceRevisionRef.current, nextRevision)
        ) {
          return;
        }

        const data = await api.loadSessionMessages(agent, sourcePath);
        setMessages((current) =>
          sessionMessagesShareTranscript(current, data) ? current : data,
        );
        sourceRevisionRef.current = nextRevision;
      } finally {
        refreshInFlightRef.current = false;
      }
    },
    [agent, sourcePath],
  );

  useEffect(() => {
    let cancelled = false;
    sourceRevisionRef.current = null;
    setLoading(true);
    refreshMessages(true)
      .catch((error) => {
        if (cancelled) {
          return;
        }
        console.warn(
          `[sessions] message initial load failed agent=${agent} source=${sourcePath}`,
          error,
        );
        setMessages([]);
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [agent, sourcePath, refreshMessages]);

  // 定时刷新消息内容（文件可能持续写入）
  useEffect(() => {
    const timer = setInterval(async () => {
      try {
        await refreshMessages(false);
      } catch (error) {
        console.warn(
          `[sessions] message refresh failed agent=${agent} source=${sourcePath}`,
          error,
        );
      }
    }, MESSAGE_REFRESH_INTERVAL);
    return () => clearInterval(timer);
  }, [agent, sourcePath, refreshMessages]);

  return { messages, loading };
}

export function useSessionRawContent(
  agent: string,
  sourcePath: string,
  enabled: boolean,
) {
  const [rawContent, setRawContent] = useState<SessionRawContent | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!enabled) return;

    setLoading(true);
    try {
      const data = await api.loadSessionRawContent(agent, sourcePath);
      setRawContent(data);
      setError(null);
    } catch (loadError) {
      console.warn(
        `[sessions] raw content load failed agent=${agent} source=${sourcePath}`,
        loadError,
      );
      setError(String(loadError));
      setRawContent(null);
    } finally {
      setLoading(false);
    }
  }, [agent, sourcePath, enabled]);

  useEffect(() => {
    if (!enabled) {
      return;
    }
    void refresh();
  }, [enabled, refresh]);

  return { rawContent, loading, error, refresh };
}
