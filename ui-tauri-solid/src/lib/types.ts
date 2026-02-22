export type AgentStatus = "idle" | "active" | "blocked" | "review_pending" | "done";

export type GateState =
  | "plan"
  | "execute"
  | "review"
  | "human_smoke_test"
  | "deploy"
  | "complete";

export interface HealthResponse {
  status: "ok";
  engine: string;
  version: string;
}

export interface SubAgent {
  id: string;
  name: string;
  task_id: string;
  objective: string;
  status: AgentStatus;
}

export interface GateConditions {
  plan_defined: boolean;
  plan_acknowledged: boolean;
  execution_artifacts_present: boolean;
  passed_reasoners_test: boolean;
  kaizen_review_approved: boolean;
  human_smoke_test_passed: boolean;
  deploy_validation_passed: boolean;
}

export interface GateSnapshot {
  current_state: GateState;
  conditions: GateConditions;
  hard_gates_enabled: boolean;
}

export interface TransitionResult {
  allowed: boolean;
  from: GateState;
  to: GateState;
  blocked_by: string[];
}

export interface CrystalBallEvent {
  event_id: string;
  timestamp: string;
  event_type: string;
  source_actor: string;
  source_agent_id: string;
  target_actor: string;
  target_agent_id: string;
  task_id: string;
  message: string;
  visibility: string;
}

export interface ChatMessage {
  role: string;
  content: string;
}

export interface ChatHistoryResponse {
  conversation_key: string;
  messages: ChatMessage[];
}

export interface ChatResponse {
  reply: string;
  source: string;
  target: string;
  active_agents: number;
  gate_state: GateState;
  model: string | null;
  provider: string | null;
  mode: string | null;
  input_tokens: number | null;
  output_tokens: number | null;
}

export interface SecretMetadata {
  provider: string;
  configured: boolean;
  key_id: string;
  created_at: string;
  last_updated: string;
  last4: string;
  secret_type: string;
}

export interface VaultStatus {
  available: boolean;
  key_source: string;
  vault_path: string;
  key_path: string | null;
  bootstrap_created: boolean;
  error: string | null;
}

export interface OAuthStatus {
  provider: string;
  supported: boolean;
  connected: boolean;
  access_token_configured: boolean;
  refresh_token_configured: boolean;
  message: string;
}

export interface GitHubStatus {
  authenticated: boolean;
  host: string;
  login: string | null;
  token_source: string | null;
  scopes: string[];
  git_protocol: string | null;
  error: string | null;
}

export interface GitHubRepoSummary {
  name_with_owner: string;
  is_private: boolean;
  updated_at: string;
  url: string;
  viewer_permission: string;
}

export interface GitHubReposResponse {
  connected: boolean;
  repos: GitHubRepoSummary[];
  error: string | null;
}

export interface CrystalBallHealth {
  enabled: boolean;
  mode: string;
  mattermost_configured: boolean;
  mattermost_connected: boolean;
  local_archive_path: string;
  local_archive_ttl_days: number;
  local_event_count: number;
  archive_integrity_valid: boolean;
  archive_hmac_configured: boolean;
  archive_signed_records: number;
  archive_legacy_unsigned_records: number;
  archive_mac_verified_records: number;
  archive_mac_missing_records: number;
  archive_mac_unverified_records: number;
  archive_last_hash: string;
}

export interface CrystalBallValidation {
  enabled: boolean;
  configured: boolean;
  validation: {
    reachable: boolean;
    auth_ok: boolean;
    channel_ok: boolean;
    user_id: string | null;
    username: string | null;
    channel_id: string;
    channel_name: string | null;
    channel_display_name: string | null;
    error: string | null;
  };
  error: string | null;
}

export interface CrystalBallSmoke {
  enabled: boolean;
  configured: boolean;
  success: boolean;
  smoke: {
    sent: boolean;
    fetched: boolean;
    detected: boolean;
    post_id: string | null;
    marker: string;
    error: string | null;
  };
  error: string | null;
}

export interface CrystalBallAudit {
  valid: boolean;
  total_records: number;
  signed_records: number;
  legacy_unsigned_records: number;
  hmac_configured: boolean;
  mac_verified_records: number;
  mac_missing_records: number;
  mac_unverified_records: number;
  first_invalid_line: number | null;
  reason: string | null;
  last_hash: string;
}

export interface KaizenSettings {
  runtime_engine: string;
  openclaw_compat_enabled: boolean;
  auto_spawn_subagents: boolean;
  max_subagents: number;
  main_chat_pinned: boolean;
  new_agent_chat_default_state: string;
  allow_direct_user_to_subagent_chat: boolean;
  crystal_ball_enabled: boolean;
  crystal_ball_default_open: boolean;
  mattermost_url: string;
  mattermost_channel_id: string;
  selected_github_repo: string;
  hard_gates_enabled: boolean;
  require_human_smoke_test_before_deploy: boolean;
  provider_inference_only: boolean;
  credentials_ui_enabled: boolean;
  agent_name_editable_after_spawn: boolean;
  secrets_storage_mode: string;
  write_plaintext_secrets_to_env: boolean;
  show_only_masked_secrets_in_ui: boolean;
  inference_provider: string;
  inference_model: string;
  inference_max_tokens: number;
  inference_temperature: number;
  [key: string]: unknown;
}

export interface Notice {
  id: string;
  kind: "info" | "success" | "warning" | "error";
  text: string;
}
