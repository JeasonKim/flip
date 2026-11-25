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
  source: SessionSourceRef | null;
  project_dir: string | null;
  last_active_at: number | null;
  source_path: string;
  source_file_path: string | null;
  resume_command: string | null;
}

export interface SessionSourceRef {
  id: string;
  label: string;
}

export interface SessionSourceConfig {
  id: string;
  label: string;
  path: string;
  enabled: boolean;
}

export interface SessionSource extends SessionSourceConfig {
  is_default: boolean;
  issue: string | null;
}

export type SessionTimelineCategory =
  | "user"
  | "assistant"
  | "reasoning"
  | "tool_call"
  | "tool_result"
  | "system"
  | "metadata"
  | "event"
  | "unknown";

export type SessionTimelineImportance = "primary" | "detail" | "debug";

export interface SessionRawReference {
  section: string;
  index: number;
  record_id: string | null;
}

export interface SessionPrompt {
  role: "system" | "developer";
  origin: "configured" | "request" | "model" | "runtime" | "snapshot" | "unknown";
  text: string;
  entrypoint: string | null;
  source: SessionRawReference;
}

export interface SessionTimelineEntry {
  id: string;
  category: SessionTimelineCategory;
  importance: SessionTimelineImportance;
  title: string;
  role: string;
  content: string;
  timestamp: number | null;
  raw_reference: SessionRawReference;
}

/// 统一口径：input 只含未命中缓存的新增输入，output 含思考产出，thinking 是其中的思考部分
export interface SessionTokenUsage {
  total_tokens: number | null;
  input_tokens: number | null;
  output_tokens: number | null;
  thinking_tokens: number | null;
  cache_read_tokens: number | null;
  cache_write_tokens: number | null;
  cache_write_1h_tokens: number | null;
  cache_write_5m_tokens: number | null;
}

export type SessionTimelineBlockKind = "context" | "turn";

export type SessionTurnStatus = "running" | "completed";

export interface SessionTimelineBlock {
  id: string;
  kind: SessionTimelineBlockKind;
  ordinal: number | null;
  status: SessionTurnStatus | null;
  usage: SessionTokenUsage | null;
  entry_start: number;
  entry_end: number;
}

export interface SessionTimeline {
  blocks: SessionTimelineBlock[];
  entries: SessionTimelineEntry[];
}

export interface SessionSourceRevision {
  size_bytes: number | null;
  modified_at: number | null;
}

export interface ModelInfo {
  model: string | null;
  thinking: string | null;
}
