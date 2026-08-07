import { describe, expect, it } from "vitest";
import type { SessionMeta, SessionMessage, SessionSourceRevision } from "../types/profile";
import {
  sessionIdentityKey,
  sessionListsShareVisibleState,
  sessionMessagesShareTranscript,
  sessionSourceRevisionChanged,
} from "./sessionRefresh";

const baseSession: SessionMeta = {
  agent: "codex",
  session_id: "s1",
  title: "Build feature",
  project_dir: "/tmp/project",
  last_active_at: 1000,
  source_path: "/tmp/project/session.jsonl",
  resume_command: "codex resume s1",
};

const baseMessage: SessionMessage = {
  role: "assistant",
  content: "done",
  timestamp: 2000,
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

  it("keeps current transcript identity when parsed messages are unchanged", () => {
    expect(
      sessionMessagesShareTranscript([baseMessage], [{ ...baseMessage }]),
    ).toBe(true);
  });

  it("detects message content changes", () => {
    expect(
      sessionMessagesShareTranscript(
        [baseMessage],
        [{ ...baseMessage, content: "updated" }],
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
