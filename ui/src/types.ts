/** Agent lifecycle status - mirrors core/src/agents.rs */
export type AgentStatus =
  | "idle"
  | "active"
  | "blocked"
  | "review_pending"
  | "done";

export interface ChatMessage {
  role: "user" | "kaizen" | "agent";
  content: string;
  timestamp: string;
}

/** Sub-agent representation */
export interface Agent {
  id: string;
  name: string;
  taskId: string;
  objective: string;
  status: AgentStatus;
  chatOpen: boolean;
  messages: ChatMessage[];
}

/** Gate states - mirrors core/src/gate_engine.rs */
export type GateState =
  | "plan"
  | "execute"
  | "review"
  | "human_smoke_test"
  | "deploy"
  | "complete";

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

export type GateConditionPatch = Partial<GateConditions>;

export interface GateTransitionResult {
  allowed: boolean;
  from: GateState;
  to: GateState;
  blocked_by: string[];
}

export interface ChatResponse {
  reply: string;
  source: string;
  target: string;
  active_agents: number;
  gate_state: GateState;
  model?: string;
  provider?: string;
  input_tokens?: number;
  output_tokens?: number;
}

/** SSE stream token event */
export interface StreamTokenEvent {
  text: string;
}

/** SSE stream done event */
export interface StreamDoneEvent {
  full_response: string;
  model: string;
  provider: string;
}

/** Crystal Ball event */
export interface CrystalBallEvent {
  eventId: string;
  timestamp: string;
  type: string;
  sourceActor: string;
  sourceAgentId: string;
  targetActor: string;
  targetAgentId: string;
  taskId: string;
  message: string;
  visibility: "operator" | "admin" | "audit";
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

export interface ArchiveIntegrityReport {
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

export interface MattermostValidation {
  reachable: boolean;
  auth_ok: boolean;
  channel_ok: boolean;
  user_id: string | null;
  username: string | null;
  channel_id: string;
  channel_name: string | null;
  channel_display_name: string | null;
  error: string | null;
}

export interface MattermostSmokeResult {
  sent: boolean;
  fetched: boolean;
  detected: boolean;
  post_id: string | null;
  marker: string;
  error: string | null;
}

export interface CrystalBallValidateResponse {
  enabled: boolean;
  configured: boolean;
  validation: MattermostValidation | null;
  error: string | null;
}

export interface CrystalBallSmokeResponse {
  enabled: boolean;
  configured: boolean;
  success: boolean;
  smoke: MattermostSmokeResult | null;
  error: string | null;
}

/** Settings from config/defaults.json */
export interface KaizenSettings {
  runtime_engine: "zeroclaw" | "openclaw_compat";
  openclaw_compat_enabled: boolean;
  auto_spawn_subagents: boolean;
  max_subagents: number;
  main_chat_pinned: boolean;
  new_agent_chat_default_state: "open" | "closed";
  allow_direct_user_to_subagent_chat: boolean;
  crystal_ball_enabled: boolean;
  crystal_ball_default_open: boolean;
  hard_gates_enabled: boolean;
  require_human_smoke_test_before_deploy: boolean;
  provider_inference_only: boolean;
  credentials_ui_enabled: boolean;
  oauth_ui_enabled: boolean;
  agent_name_editable_after_spawn: boolean;
  secrets_storage_mode: string;
  write_plaintext_secrets_to_env: boolean;
  show_only_masked_secrets_in_ui: boolean;
  inference_provider: string;
  inference_model: string;
  inference_max_tokens: number;
  inference_temperature: number;
}

export type KaizenSettingsPatch = Partial<KaizenSettings>;

/** Secret vault metadata (never contains raw values) */
export interface SecretMetadata {
  provider: string;
  configured: boolean;
  last_updated: string;
  last4: string;
  secret_type: string;
}

export interface SecretTestResult {
  provider: string;
  configured: boolean;
  test_passed: boolean;
  error: string | null;
}
