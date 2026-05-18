export type AccountType = "plan" | "api";
export type AgentId = "claude" | "codex";
/// 会话查看支持的 agent（只读，不涉及账号切换）
export type SessionAgentId = "claude" | "codex" | "opencode";

export interface Account {
  id: string;
  type: AccountType;
  label: string;
  credentials?: Record<string, unknown>;
  auth?: Record<string, unknown>;
  config?: string;
}

export interface AgentConfig {
  current: string | null;
  accounts: Account[];
}

export interface FlipConfig {
  version: number;
  claude: AgentConfig;
  codex: AgentConfig;
}

export interface QuotaTier {
  name: string;
  utilization: number;
  resets_at: string | null;
}

export interface QuotaResult {
  success: boolean;
  tiers: QuotaTier[];
  error: string | null;
}

export interface SessionMeta {
  agent: string;
  session_id: string;
  title: string;
  project_dir: string | null;
  last_active_at: number | null;
  source_path: string;
  resume_command: string | null;
}

export interface SessionMessage {
  role: string;
  content: string;
  timestamp: number | null;
}

export interface ModelInfo {
  model: string | null;
  thinking: string | null;
}
