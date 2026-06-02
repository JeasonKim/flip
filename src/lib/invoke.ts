import { invoke } from "@tauri-apps/api/core";
import type {
  AgentId,
  SessionAgentId,
  FlipConfig,
  Account,
  QuotaResult,
  ModelInfo,
  SessionMeta,
  SessionMessage,
  SessionRawContent,
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
): Promise<SessionMeta[]> {
  return invoke<SessionMeta[]>("scan_sessions", {
    agentFilter: agentFilter ?? null,
    offset,
    limit,
  });
}

export async function loadSessionMessages(
  agent: string,
  sourcePath: string,
): Promise<SessionMessage[]> {
  return invoke<SessionMessage[]>("load_session_messages", {
    agent,
    sourcePath,
  });
}

export async function loadSessionRawContent(
  agent: string,
  sourcePath: string,
): Promise<SessionRawContent> {
  return invoke<SessionRawContent>("load_session_raw_content", {
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
