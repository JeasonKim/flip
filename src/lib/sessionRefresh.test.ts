import { describe, expect, it } from "vitest";
import type {
  SessionMeta,
  SessionSourceRevision,
  SessionTimeline,
  SessionTimelineEntry,
} from "../types/profile";
import {
  sessionIdentityKey,
  sessionListsShareVisibleState,
  sessionTimelinesShareVisibleState,
  sessionSourceRevisionChanged,
} from "./sessionRefresh";

const baseSession: SessionMeta = {
  agent: "codex",
  session_id: "s1",
  title: "Build feature",
  source: null,
  project_dir: "/tmp/project",
  last_active_at: 1000,
  source_path: "/tmp/project/session.jsonl",
  source_file_path: "/tmp/project/session.jsonl",
  resume_command: "codex resume s1",
};

const baseEntry: SessionTimelineEntry = {
  id: "jsonl-1",
  category: "assistant",
  importance: "primary",
  title: "Assistant",
  role: "assistant",
  content: "done",
  timestamp: 2000,
  raw_reference: {
    section: "jsonl",
    index: 1,
    record_id: null,
  },
};

const baseTimeline: SessionTimeline = {
  blocks: [
    {
      id: "turn-1-jsonl-1",
      kind: "turn",
      ordinal: 1,
      status: "completed",
      usage: {
        total_tokens: 20,
        input_tokens: 15,
        output_tokens: 5,
        cache_read_tokens: 10,
        thinking_tokens: 0,
    cache_write_tokens: 0,
    cache_write_1h_tokens: 0,
    cache_write_5m_tokens: 0,
      },
      entry_start: 0,
      entry_end: 1,
    },
  ],
  entries: [baseEntry],
};

const baseRevision: SessionSourceRevision = {
  size_bytes: 10,
  modified_at: 3000,
};

describe("session refresh", () => {
  it("distinguishes forked sessions that share a thread id", () => {
    expect(
      sessionIdentityKey({
        agent: "codex",
        source_path: "/tmp/rollout-current.jsonl",
      }),
    ).not.toBe(
      sessionIdentityKey({
        agent: "codex",
        source_path: "/tmp/rollout-ancestor.jsonl",
      }),
    );
  });

  it("keeps current session list identity when visible fields are unchanged", () => {
    expect(
      sessionListsShareVisibleState([baseSession], [{ ...baseSession }]),
    ).toBe(true);
  });

  it("detects session list changes that should update the sidebar", () => {
    expect(
      sessionListsShareVisibleState(
        [baseSession],
        [{ ...baseSession, title: "New title" }],
      ),
    ).toBe(false);
  });

  it("refreshes source labels without changing session identity", () => {
    const current = { ...baseSession, source: { id: "dev", label: "开发" } };
    const renamed = { ...current, source: { id: "dev", label: "SkillFlow Dev" } };
    expect(sessionIdentityKey(current)).toBe(sessionIdentityKey(renamed));
    expect(sessionListsShareVisibleState([current], [renamed])).toBe(false);
  });

  it("detects when a session gains or loses a revealable source file", () => {
    expect(
      sessionListsShareVisibleState(
        [baseSession],
        [{ ...baseSession, source_file_path: null }],
      ),
    ).toBe(false);
  });

  it("keeps current transcript identity when parsed messages are unchanged", () => {
    expect(
      sessionTimelinesShareVisibleState(baseTimeline, {
        ...baseTimeline,
        entries: [{ ...baseEntry }],
      }),
    ).toBe(true);
  });

  it("detects message content changes", () => {
    expect(
      sessionTimelinesShareVisibleState(
        baseTimeline,
        {
          ...baseTimeline,
          entries: [{ ...baseEntry, content: "updated" }],
        },
      ),
    ).toBe(false);
  });

  it("detects message importance changes", () => {
    expect(
      sessionTimelinesShareVisibleState(baseTimeline, {
        ...baseTimeline,
        entries: [{ ...baseEntry, importance: "detail" }],
      }),
    ).toBe(false);
  });

  it("detects a running turn becoming completed with final usage", () => {
    expect(
      sessionTimelinesShareVisibleState(
        {
          ...baseTimeline,
          blocks: [
            {
              ...baseTimeline.blocks[0],
              status: "running",
              usage: null,
            },
          ],
        },
        baseTimeline,
      ),
    ).toBe(false);
  });

  it("treats matching source revisions as unchanged", () => {
    expect(
      sessionSourceRevisionChanged(baseRevision, { ...baseRevision }),
    ).toBe(false);
  });

  it("treats missing or changed source revisions as needing a refresh", () => {
    expect(sessionSourceRevisionChanged(null, baseRevision)).toBe(true);
    expect(
      sessionSourceRevisionChanged(baseRevision, {
        ...baseRevision,
        size_bytes: 11,
      }),
    ).toBe(true);
  });
});
