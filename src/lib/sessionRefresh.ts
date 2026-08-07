import type { SessionMeta, SessionMessage, SessionSourceRevision } from "../types/profile";

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
      currentSession.project_dir === nextSession.project_dir &&
      currentSession.last_active_at === nextSession.last_active_at &&
      currentSession.source_path === nextSession.source_path &&
      currentSession.resume_command === nextSession.resume_command
    );
  });
}

export function sessionMessagesShareTranscript(
  currentMessages: SessionMessage[],
  nextMessages: SessionMessage[],
): boolean {
  if (currentMessages.length !== nextMessages.length) {
    return false;
  }

  return currentMessages.every((currentMessage, index) => {
    const nextMessage = nextMessages[index];
    return (
      currentMessage.role === nextMessage.role &&
      currentMessage.timestamp === nextMessage.timestamp &&
      currentMessage.content === nextMessage.content
    );
  });
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
