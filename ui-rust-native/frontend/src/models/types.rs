use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabId {
    Mission,
    Branches,
    Gates,
    Activity,
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
    pub task_id: Option<String>,
    pub objective: String,
    pub status: AgentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    Idle,
    Active,
    Blocked,
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
pub struct CompanyBranch {
    pub id: String,
    pub name: String,
    pub location: String,
    pub status: String,
    pub manager_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mission {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worker {
    pub id: String,
    pub name: String,
    pub role: String,
    pub active: bool,
    pub branch_id: Option<String>,
}
