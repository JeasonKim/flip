import { invoke } from "@tauri-apps/api/core";
import type {
  AgentId,
  SessionAgentId,
  FlipConfig,
  Account,
  QuotaResult,
  ModelInfo,
  SessionMeta,
  SessionPrompt,
  SessionTimeline,
  SessionRawReference,
  SessionSourceRevision,
  SessionSource,
  SessionSourceConfig,
} from "../types/profile";

export async function listProfiles(): Promise<FlipConfig> {
  return invoke<FlipConfig>("list_profiles");
}

export async function reconcileLiveProfiles(): Promise<FlipConfig> {
  return invoke<FlipConfig>("reconcile_live_profiles");
}

export async function flipAccount(
  agent: AgentId,
  accountId: string,
): Promise<void> {
  return invoke("flip_account", { agent, accountId });
}

export async function captureCurrentAccount(
  agent: AgentId,
): Promise<Account> {
  return invoke<Account>("capture_current", { agent });
}

export async function dismissAccount(
  agent: AgentId,
  accountId: string,
): Promise<void> {
  return invoke("dismiss_account", { agent, accountId });
}

export async function syncCredentials(agent: AgentId): Promise<void> {
  return invoke("sync_credentials", { agent });
}

export async function detectUnsaved(agent: AgentId): Promise<boolean> {
  return invoke<boolean>("detect_unsaved", { agent });
}

export async function fetchQuota(agent: AgentId): Promise<QuotaResult> {
  return invoke<QuotaResult>("fetch_quota", { agent });
}

export async function readModelInfo(agent: AgentId): Promise<ModelInfo> {
  return invoke<ModelInfo>("read_model_info", { agent });
}

export async function renameAccount(
  agent: AgentId,
  accountId: string,
  newLabel: string,
): Promise<void> {
  return invoke("rename_account", { agent, accountId, newLabel });
}

export async function scanSessions(
  agentFilter?: SessionAgentId,
  offset = 0,
  limit = 20,
  sourceFilter?: string,
): Promise<SessionMeta[]> {
  return invoke<SessionMeta[]>("scan_sessions", {
    agentFilter: agentFilter ?? null,
    sourceFilter: sourceFilter ?? null,
    offset,
    limit,
  });
}

export async function listSessionSources(): Promise<SessionSource[]> {
  return invoke<SessionSource[]>("list_session_sources");
}

export async function configureSessionSources(sources: SessionSourceConfig[]): Promise<SessionSource[]> {
  return invoke<SessionSource[]>("configure_session_sources", { sources });
}

export async function loadSessionTimeline(
  agent: string,
  sourcePath: string,
): Promise<SessionTimeline> {
  return invoke<SessionTimeline>("load_session_timeline", {
    agent,
    sourcePath,
  });
}

export function loadSessionSystemPrompts(
  agent: string,
  sourcePath: string,
): Promise<SessionPrompt[]> {
  return invoke("load_session_system_prompts", { agent, sourcePath });
}

export async function loadSessionEntryRaw(
  agent: string,
  sourcePath: string,
  rawReference: SessionRawReference,
): Promise<unknown> {
  return invoke<unknown>("load_session_entry_raw", {
    agent,
    sourcePath,
    rawReference,
  });
}

export async function openSessionResource(
  target: string,
  projectDir?: string,
): Promise<void> {
  return invoke("open_session_resource", {
    target,
    projectDir: projectDir ?? null,
  });
}

export async function revealSessionSource(
  agent: string,
  sourceFilePath: string,
): Promise<void> {
  return invoke("reveal_session_source", { agent, sourceFilePath });
}

export async function readSessionSourceRevision(
  agent: string,
  sourcePath: string,
): Promise<SessionSourceRevision> {
  return invoke<SessionSourceRevision>("read_session_source_revision", {
    agent,
    sourcePath,
  });
}

export async function resumeSession(
  command: string,
  cwd?: string,
): Promise<void> {
  return invoke("resume_session", { command, cwd: cwd ?? null });
}

export async function purgeSessions(olderThanDays: number): Promise<number> {
  return invoke<number>("purge_sessions", { olderThanDays });
}

export async function importFromCcswitch(): Promise<{
  imported: number;
  skipped: number;
}> {
  return invoke("import_from_ccswitch");
}

export async function enrollApiAccount(
  agent: AgentId,
  label: string,
  apiKey: string,
  baseUrl?: string,
): Promise<Account> {
  return invoke<Account>("enroll_api_account", {
    agent,
    label,
    apiKey,
    baseUrl: baseUrl ?? null,
  });
}

export async function openSessionWindow(): Promise<void> {
  return invoke("open_session_window");
}

export async function revealConfigFile(): Promise<void> {
  return invoke("reveal_config_file");
}
