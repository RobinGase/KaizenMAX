use serde::{Deserialize, Serialize};

fn default_primary_branch() -> String {
    "primary".to_string()
}

fn default_general_mission() -> String {
    "general".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabId {
    Mission,
    Branches,
    Gates,
    Activity,
    Memory,
    Calendar,
    Kanban,
    Workspace,
    Integrations,
    Settings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub engine: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubAgent {
    pub id: String,
    pub name: String,
    #[serde(default = "default_primary_branch")]
    pub branch_id: String,
    #[serde(default = "default_general_mission")]
    pub mission_id: String,
    pub task_id: String,
    pub objective: String,
    pub status: AgentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchStatus {
    Active,
    Paused,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    Backlog,
    InProgress,
    Review,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    pub id: String,
    pub name: String,
    pub status: BranchStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mission {
    pub id: String,
    pub branch_id: String,
    pub name: String,
    pub objective: String,
    pub status: MissionStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionTopologyNode {
    pub mission: Mission,
    pub workers: Vec<SubAgent>,
    pub active_workers: usize,
    pub blocked_workers: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchTopologyNode {
    pub branch: Branch,
    pub missions: Vec<MissionTopologyNode>,
    pub total_workers: usize,
    pub active_workers: usize,
    pub blocked_workers: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Active,
    Blocked,
    ReviewPending,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrystalBallEvent {
    pub event_id: String,
    pub timestamp: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub source_actor: String,
    pub source_agent_id: String,
    pub target_actor: String,
    pub target_agent_id: String,
    pub task_id: String,
    pub message: String,
    pub visibility: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InferenceChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatHistoryResponse {
    pub conversation_key: String,
    pub messages: Vec<InferenceChatMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatResponse {
    pub reply: String,
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateState {
    Plan,
    Execute,
    Review,
    HumanSmokeTest,
    Deploy,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateConditions {
    pub plan_defined: bool,
    pub plan_acknowledged: bool,
    pub execution_artifacts_present: bool,
    pub passed_reasoners_test: bool,
    pub kaizen_review_approved: bool,
    pub human_smoke_test_passed: bool,
    pub deploy_validation_passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateSnapshot {
    pub current_state: GateState,
    pub conditions: GateConditions,
    pub hard_gates_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateTransitionResult {
    pub allowed: bool,
    pub from: GateState,
    pub to: GateState,
    pub blocked_by: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KaizenSettings {
    pub runtime_engine: String,
    pub openclaw_compat_enabled: bool,
    pub auto_spawn_subagents: bool,
    pub orchestrator_full_control: bool,
    pub max_subagents: u32,
    pub main_chat_pinned: bool,
    pub new_agent_chat_default_state: String,
    pub allow_direct_user_to_subagent_chat: bool,
    pub crystal_ball_enabled: bool,
    pub crystal_ball_default_open: bool,
    pub hard_gates_enabled: bool,
    pub require_human_smoke_test_before_deploy: bool,
    pub provider_inference_only: bool,
    pub credentials_ui_enabled: bool,
    pub agent_name_editable_after_spawn: bool,
    pub secrets_storage_mode: String,
    pub write_plaintext_secrets_to_env: bool,
    pub show_only_masked_secrets_in_ui: bool,
    pub mattermost_url: String,
    pub mattermost_channel_id: String,
    pub selected_github_repo: String,
    pub inference_provider: String,
    pub inference_model: String,
    pub inference_max_tokens: u32,
    pub inference_temperature: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubStatusResponse {
    pub authenticated: bool,
    pub host: String,
    pub login: Option<String>,
    pub token_source: Option<String>,
    pub scopes: Vec<String>,
    pub git_protocol: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubRepoSummary {
    pub name_with_owner: String,
    pub is_private: bool,
    pub updated_at: String,
    pub url: String,
    pub viewer_permission: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubReposResponse {
    pub connected: bool,
    pub repos: Vec<GitHubRepoSummary>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultStatusResponse {
    pub available: bool,
    pub key_source: String,
    pub vault_path: String,
    pub key_path: Option<String>,
    pub bootstrap_created: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretMetadata {
    pub provider: String,
    pub configured: bool,
    pub key_id: String,
    pub created_at: String,
    pub last_updated: String,
    pub last4: String,
    pub secret_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretTestResponse {
    pub provider: String,
    pub configured: bool,
    pub test_passed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthStatusResponse {
    pub provider: String,
    pub supported: bool,
    pub connected: bool,
    pub access_token_configured: bool,
    pub refresh_token_configured: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthStartResponse {
    pub provider: String,
    pub redirect_url: String,
    pub state_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderAuthStatusResponse {
    pub provider: String,
    pub resolved_provider: String,
    pub native_alias: bool,
    pub auth_method: String,
    pub configured: bool,
    pub can_chat: bool,
    pub message: String,
    pub env_hints: Vec<String>,
}
