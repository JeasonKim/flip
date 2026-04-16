import { useCallback, useEffect, useRef, useState } from "react";
import type { AgentId, SessionMeta, SessionMessage } from "../types/profile";
import * as api from "../lib/invoke";

const PAGE_SIZE = 20;

const REFRESH_INTERVAL = 10000;

export function useSessions() {
  const [sessions, setSessions] = useState<SessionMeta[]>([]);
  const [projects, setProjects] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [hasMore, setHasMore] = useState(true);

  // 筛选条件
  const [agentFilter, setAgentFilter] = useState<AgentId | undefined>();
  const [projectFilter, setProjectFilter] = useState<string | undefined>();

  // 当前已加载的总条数，用于静默刷新时保持范围一致
  const loadedCountRef = useRef(PAGE_SIZE);

  const loadInitial = useCallback(async () => {
    setLoading(true);
    try {
      const [data, allProjects] = await Promise.all([
        api.scanSessions(agentFilter, projectFilter, 0, PAGE_SIZE),
        api.listSessionProjects(),
      ]);
      setSessions(data);
      setProjects(allProjects);
      setHasMore(data.length >= PAGE_SIZE);
      loadedCountRef.current = data.length;
    } finally {
      setLoading(false);
    }
  }, [agentFilter, projectFilter]);

  const loadMore = useCallback(async () => {
    const data = await api.scanSessions(
      agentFilter,
      projectFilter,
      sessions.length,
      PAGE_SIZE,
    );
    setSessions((prev) => [...prev, ...data]);
    setHasMore(data.length >= PAGE_SIZE);
    loadedCountRef.current = sessions.length + data.length;
  }, [agentFilter, projectFilter, sessions.length]);

  useEffect(() => {
    loadInitial();
  }, [loadInitial]);

  // 定时静默刷新：不触发 loading，不影响选中状态
  useEffect(() => {
    const timer = setInterval(async () => {
      try {
        const count = Math.max(loadedCountRef.current, PAGE_SIZE);
        const data = await api.scanSessions(agentFilter, projectFilter, 0, count);
        setSessions(data);
        setHasMore(data.length >= count);
      } catch {
        // 静默刷新失败不中断
      }
    }, REFRESH_INTERVAL);
    return () => clearInterval(timer);
  }, [agentFilter, projectFilter]);

  return {
    sessions,
    projects,
    loading,
    hasMore,
    agentFilter,
    setAgentFilter,
    projectFilter,
    setProjectFilter,
    loadMore,
    refresh: loadInitial,
  };
}

const MESSAGE_REFRESH_INTERVAL = 3000;

export function useSessionMessages(agent: string, sourcePath: string) {
  const [messages, setMessages] = useState<SessionMessage[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading(true);
    api
      .loadSessionMessages(agent, sourcePath)
      .then(setMessages)
      .finally(() => setLoading(false));
  }, [agent, sourcePath]);

  // 定时刷新消息内容（文件可能持续写入）
  useEffect(() => {
    const timer = setInterval(async () => {
      try {
        const data = await api.loadSessionMessages(agent, sourcePath);
        setMessages(data);
      } catch {
        // 静默失败
      }
    }, MESSAGE_REFRESH_INTERVAL);
    return () => clearInterval(timer);
  }, [agent, sourcePath]);

  return { messages, loading };
}
