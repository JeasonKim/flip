import { useCallback, useEffect, useRef, useState } from "react";
import type {
  SessionAgentId,
  SessionMeta,
  SessionTimeline,
  SessionSourceRevision,
  SessionSource,
} from "../types/profile";
import * as api from "../lib/invoke";
import {
  sessionListsShareVisibleState,
  sessionTimelinesShareVisibleState,
  sessionSourceRevisionChanged,
} from "../lib/sessionRefresh";

const PAGE_SIZE = 20;

const REFRESH_INTERVAL = 10000;

export function useSessions() {
  const [sessions, setSessions] = useState<SessionMeta[]>([]);
  const [sources, setSources] = useState<SessionSource[]>([]);
  const [sourceError, setSourceError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [hasMore, setHasMore] = useState(true);
  const [agentFilter, changeAgentFilter] = useState<SessionAgentId | undefined>();
  const [sourceFilter, setSourceFilter] = useState<string | undefined>();
  const loadedCountRef = useRef(PAGE_SIZE);
  const queryVersionRef = useRef(0);
  const refreshInFlightRef = useRef(false);
  const moreInFlightRef = useRef(false);

  const setAgentFilter = useCallback((agent: SessionAgentId | undefined) => {
    changeAgentFilter(agent);
    if (agent && agent !== "codex") setSourceFilter(undefined);
  }, []);

  const loadInitial = useCallback(async () => {
    const version = ++queryVersionRef.current;
    setLoading(true);
    setError(null);
    try {
      const data = await api.scanSessions(agentFilter, 0, PAGE_SIZE, sourceFilter);
      if (version !== queryVersionRef.current) return;
      setSessions((current) => sessionListsShareVisibleState(current, data) ? current : data);
      setHasMore(data.length >= PAGE_SIZE);
      loadedCountRef.current = data.length;
    } catch (cause) {
      console.warn("[sessions] initial scan failed", { agentFilter, sourceFilter }, cause);
      if (version === queryVersionRef.current) {
        setSessions([]);
        setError(String(cause));
      }
    } finally {
      if (version === queryVersionRef.current) setLoading(false);
    }
  }, [agentFilter, sourceFilter]);

  const loadMore = useCallback(async () => {
    if (moreInFlightRef.current || refreshInFlightRef.current || loading) return;
    const version = queryVersionRef.current;
    moreInFlightRef.current = true;
    try {
      const data = await api.scanSessions(agentFilter, sessions.length, PAGE_SIZE, sourceFilter);
      if (version !== queryVersionRef.current) return;
      setSessions((previous) => [...previous, ...data]);
      setHasMore(data.length >= PAGE_SIZE);
      loadedCountRef.current = sessions.length + data.length;
      setError(null);
    } catch (cause) {
      console.warn("[sessions] pagination failed", { agentFilter, sourceFilter }, cause);
      if (version === queryVersionRef.current) setError(String(cause));
    } finally {
      moreInFlightRef.current = false;
    }
  }, [agentFilter, sourceFilter, sessions.length, loading]);

  useEffect(() => {
    let cancelled = false;
    api.listSessionSources().then((data) => {
      if (!cancelled) setSources(data);
    }).catch((cause) => {
      console.warn("[session-sources] listing sources failed", cause);
      if (!cancelled) setSourceError(String(cause));
    });
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    void loadInitial();
    return () => { queryVersionRef.current += 1; };
  }, [loadInitial]);

  useEffect(() => {
    const timer = setInterval(async () => {
      if (refreshInFlightRef.current || moreInFlightRef.current) return;
      const version = queryVersionRef.current;
      refreshInFlightRef.current = true;
      try {
        const count = Math.max(loadedCountRef.current, PAGE_SIZE);
        const data = await api.scanSessions(agentFilter, 0, count, sourceFilter);
        if (version !== queryVersionRef.current) return;
        setSessions((current) => sessionListsShareVisibleState(current, data) ? current : data);
        setHasMore(data.length >= count);
        setError(null);
      } catch (cause) {
        console.warn("[sessions] refresh failed", { agentFilter, sourceFilter }, cause);
        if (version === queryVersionRef.current) setError(String(cause));
      } finally {
        refreshInFlightRef.current = false;
      }
    }, REFRESH_INTERVAL);
    return () => clearInterval(timer);
  }, [agentFilter, sourceFilter]);

  const acceptSources = useCallback((next: SessionSource[]) => {
    setSources(next);
    setSourceError(null);
    if (sourceFilter && !next.some((source) => source.id === sourceFilter && source.enabled)) {
      console.info("[session-sources] active source disabled or removed; showing all sources", { previous: sourceFilter, next: "all", reason: "source configuration changed" });
      setSourceFilter(undefined);
    } else {
      void loadInitial();
    }
  }, [sourceFilter, loadInitial]);

  const retryCollection = useCallback(async () => {
    try {
      acceptSources(await api.listSessionSources());
    } catch (cause) {
      console.warn("[session-sources] retry failed", cause);
      setSourceError(String(cause));
      void loadInitial();
    }
  }, [acceptSources, loadInitial]);

  return {
    sessions, sources, sourceError, error, loading, hasMore,
    agentFilter, sourceFilter, setAgentFilter, setSourceFilter,
    loadMore, refresh: retryCollection, acceptSources,
  };
}

const TIMELINE_REFRESH_INTERVAL = 3000;

const EMPTY_SESSION_TIMELINE: SessionTimeline = {
  blocks: [],
  entries: [],
};

export function useSessionTimeline(agent: string, sourcePath: string) {
  const [timeline, setTimeline] = useState<SessionTimeline>(
    EMPTY_SESSION_TIMELINE,
  );
  const [loading, setLoading] = useState(true);
  const sourceRevisionRef = useRef<SessionSourceRevision | null>(null);
  const refreshInFlightRef = useRef(false);

  const refreshTimeline = useCallback(
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
          `[sessions] timeline source revision failed agent=${agent} source=${sourcePath}; falling back to full reload`,
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

        const data = await api.loadSessionTimeline(agent, sourcePath);
        setTimeline((current) =>
          sessionTimelinesShareVisibleState(current, data) ? current : data,
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
    refreshTimeline(true)
      .catch((error) => {
        if (cancelled) {
          return;
        }
        console.warn(
          `[sessions] timeline initial load failed agent=${agent} source=${sourcePath}`,
          error,
        );
        setTimeline(EMPTY_SESSION_TIMELINE);
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [agent, sourcePath, refreshTimeline]);

  // 定时刷新时间线（源文件或数据库可能持续写入）
  useEffect(() => {
    const timer = setInterval(async () => {
      try {
        await refreshTimeline(false);
      } catch (error) {
        console.warn(
          `[sessions] timeline refresh failed agent=${agent} source=${sourcePath}`,
          error,
        );
      }
    }, TIMELINE_REFRESH_INTERVAL);
    return () => clearInterval(timer);
  }, [agent, sourcePath, refreshTimeline]);

  return { timeline, entries: timeline.entries, loading };
}
