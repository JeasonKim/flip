import type {
  SessionMeta,
  SessionSourceRevision,
  SessionTimeline,
  SessionTimelineBlock,
  SessionTimelineEntry,
  SessionTokenUsage,
} from "../types/profile";

export function sessionIdentityKey(
  session: Pick<SessionMeta, "agent" | "source_path">,
): string {
  return `${session.agent}:${session.source_path}`;
}

export function sessionListsShareVisibleState(
  currentSessions: SessionMeta[],
  nextSessions: SessionMeta[],
): boolean {
  if (currentSessions.length !== nextSessions.length) {
    return false;
  }

  return currentSessions.every((currentSession, index) => {
    const nextSession = nextSessions[index];
    return (
      currentSession.agent === nextSession.agent &&
      currentSession.session_id === nextSession.session_id &&
      currentSession.title === nextSession.title &&
      currentSession.source?.id === nextSession.source?.id &&
      currentSession.source?.label === nextSession.source?.label &&
      currentSession.project_dir === nextSession.project_dir &&
      currentSession.last_active_at === nextSession.last_active_at &&
      currentSession.source_path === nextSession.source_path &&
      currentSession.source_file_path === nextSession.source_file_path &&
      currentSession.resume_command === nextSession.resume_command
    );
  });
}

export function sessionTimelinesShareVisibleState(
  currentTimeline: SessionTimeline,
  nextTimeline: SessionTimeline,
): boolean {
  const currentEntries = currentTimeline.entries;
  const nextEntries = nextTimeline.entries;
  if (currentEntries.length !== nextEntries.length) {
    return false;
  }

  return (
    currentEntries.every((currentEntry, index) =>
      timelineEntriesShareVisibleState(currentEntry, nextEntries[index]),
    ) &&
    timelineBlocksShareVisibleState(
      currentTimeline.blocks,
      nextTimeline.blocks,
    )
  );
}

function timelineEntriesShareVisibleState(
  currentEntry: SessionTimelineEntry,
  nextEntry: SessionTimelineEntry,
): boolean {
  return (
    currentEntry.id === nextEntry.id &&
    currentEntry.category === nextEntry.category &&
    currentEntry.importance === nextEntry.importance &&
    currentEntry.title === nextEntry.title &&
    currentEntry.role === nextEntry.role &&
    currentEntry.timestamp === nextEntry.timestamp &&
    currentEntry.content === nextEntry.content &&
    currentEntry.raw_reference.section === nextEntry.raw_reference.section &&
    currentEntry.raw_reference.index === nextEntry.raw_reference.index &&
    currentEntry.raw_reference.record_id === nextEntry.raw_reference.record_id
  );
}

function timelineBlocksShareVisibleState(
  currentBlocks: SessionTimelineBlock[],
  nextBlocks: SessionTimelineBlock[],
): boolean {
  if (currentBlocks.length !== nextBlocks.length) return false;
  return currentBlocks.every((currentBlock, index) => {
    const nextBlock = nextBlocks[index];
    return (
      currentBlock.id === nextBlock.id &&
      currentBlock.kind === nextBlock.kind &&
      currentBlock.ordinal === nextBlock.ordinal &&
      currentBlock.status === nextBlock.status &&
      currentBlock.entry_start === nextBlock.entry_start &&
      currentBlock.entry_end === nextBlock.entry_end &&
      tokenUsagesShareVisibleState(currentBlock.usage, nextBlock.usage)
    );
  });
}

function tokenUsagesShareVisibleState(
  currentUsage: SessionTokenUsage | null,
  nextUsage: SessionTokenUsage | null,
): boolean {
  if (currentUsage === nextUsage) return true;
  if (!currentUsage || !nextUsage) return false;
  return (
    currentUsage.total_tokens === nextUsage.total_tokens &&
    currentUsage.input_tokens === nextUsage.input_tokens &&
    currentUsage.output_tokens === nextUsage.output_tokens &&
    currentUsage.cache_read_tokens === nextUsage.cache_read_tokens &&
    currentUsage.cache_write_tokens === nextUsage.cache_write_tokens
  );
}

export function sessionSourceRevisionChanged(
  currentRevision: SessionSourceRevision | null,
  nextRevision: SessionSourceRevision | null,
): boolean {
  if (!currentRevision || !nextRevision) {
    return true;
  }

  return (
    currentRevision.size_bytes !== nextRevision.size_bytes ||
    currentRevision.modified_at !== nextRevision.modified_at
  );
}
