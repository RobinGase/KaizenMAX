//! Agent lifecycle management
//!
//! Handles sub-agent spawning, status tracking, and lifecycle transitions
//! per the agent_control_policy.yaml.

use serde::{Deserialize, Serialize};

/// Branch lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchStatus {
    Active,
    Paused,
    Archived,
}

/// Mission lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    Backlog,
    InProgress,
    Review,
    Done,
}

/// Company branch metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Branch {
    pub id: String,
    pub name: String,
    pub status: BranchStatus,
}

/// Mission metadata owned by a branch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mission {
    pub id: String,
    pub branch_id: String,
    pub name: String,
    pub objective: String,
    pub status: MissionStatus,
}

/// Agent lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Active,
    Blocked,
    ReviewPending,
    Done,
}

/// Represents a sub-agent in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgent {
    pub id: String,
    pub name: String,
    pub branch_id: String,
    pub mission_id: String,
    /// Legacy compatibility field. Defaults to `mission_id` when omitted, but may differ.
    pub task_id: String,
    pub objective: String,
    pub status: AgentStatus,
}

/// Agent registry that enforces spawn limits and lifecycle rules.
#[derive(Debug)]
pub struct AgentRegistry {
    agents: Vec<SubAgent>,
    branches: Vec<Branch>,
    missions: Vec<Mission>,
    max_subagents: usize,
}

impl AgentRegistry {
    pub fn new(max_subagents: usize) -> Self {
        let default_branch = Branch {
            id: "primary".to_string(),
            name: "Primary".to_string(),
            status: BranchStatus::Active,
        };

        Self {
            agents: Vec::new(),
            branches: vec![default_branch],
            missions: Vec::new(),
            max_subagents,
        }
    }

    pub fn set_max_subagents(&mut self, max_subagents: usize) {
        self.max_subagents = max_subagents;
    }

    pub fn list_branches(&self) -> &[Branch] {
        &self.branches
    }

    pub fn list_missions(&self) -> &[Mission] {
        &self.missions
    }

    pub fn list_missions_for_branch(&self, branch_id: &str) -> Vec<Mission> {
        let branch_id = branch_id.trim().to_ascii_lowercase();
        self.missions
            .iter()
            .filter(|mission| mission.branch_id == branch_id)
            .cloned()
            .collect()
    }

    pub fn create_branch(&mut self, id: String, name: String) -> Result<Branch, String> {
        let branch_id = id.trim().to_ascii_lowercase();
        if branch_id.is_empty() {
            return Err("Branch id cannot be empty".to_string());
        }

        if self.branches.iter().any(|branch| branch.id == branch_id) {
            return Err(format!("Branch '{}' already exists", branch_id));
        }

        let branch = Branch {
            id: branch_id,
            name: name.trim().to_string(),
            status: BranchStatus::Active,
        };
        self.branches.push(branch.clone());
        Ok(branch)
    }

    pub fn create_mission(
        &mut self,
        id: String,
        branch_id: String,
        name: String,
        objective: String,
    ) -> Result<Mission, String> {
        let mission_id = id.trim().to_string();
        if mission_id.is_empty() {
            return Err("Mission id cannot be empty".to_string());
        }

        let branch_id = branch_id.trim().to_ascii_lowercase();
        if !self.branches.iter().any(|branch| branch.id == branch_id) {
            return Err(format!("Branch '{}' does not exist", branch_id));
        }

        if self
            .missions
            .iter()
            .any(|mission| mission.id == mission_id && mission.branch_id == branch_id)
        {
            return Err(format!(
                "Mission '{}' already exists in branch '{}'",
                mission_id, branch_id
            ));
        }

        let mission = Mission {
            id: mission_id,
            branch_id,
            name: name.trim().to_string(),
            objective: objective.trim().to_string(),
            status: MissionStatus::Backlog,
        };

        self.missions.push(mission.clone());
        Ok(mission)
    }

    fn ensure_branch(&mut self, branch_id: &str) -> Result<(), String> {
        let normalized = branch_id.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Err("Branch id cannot be empty".to_string());
        }

        if self.branches.iter().any(|branch| branch.id == normalized) {
            return Ok(());
        }

        let pretty_name = normalized
            .split(['-', '_'])
            .filter(|segment| !segment.is_empty())
            .map(|segment| {
                let mut chars = segment.chars();
                match chars.next() {
                    Some(first) => {
                        let mut out = first.to_ascii_uppercase().to_string();
                        out.push_str(chars.as_str());
                        out
                    }
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        self.branches.push(Branch {
            id: normalized,
            name: if pretty_name.is_empty() {
                "Primary".to_string()
            } else {
                pretty_name
            },
            status: BranchStatus::Active,
        });

        Ok(())
    }

    fn ensure_mission(
        &mut self,
        mission_id: &str,
        branch_id: &str,
        objective: &str,
    ) -> Result<(), String> {
        let mission_id = mission_id.trim();
        if mission_id.is_empty() {
            return Err("Mission id cannot be empty".to_string());
        }

        if self
            .missions
            .iter()
            .any(|mission| mission.id == mission_id && mission.branch_id == branch_id)
        {
            return Ok(());
        }

        self.missions.push(Mission {
            id: mission_id.to_string(),
            branch_id: branch_id.to_string(),
            name: mission_id.to_string(),
            objective: objective.to_string(),
            status: MissionStatus::Backlog,
        });

        Ok(())
    }

    /// Count currently active (non-done) agents.
    pub fn active_count(&self) -> usize {
        self.agents
            .iter()
            .filter(|a| a.status != AgentStatus::Done)
            .count()
    }

    /// Attempt to spawn a new sub-agent. Returns error if limit exceeded.
    pub fn spawn(
        &mut self,
        id: String,
        name: String,
        task_id: String,
        objective: String,
    ) -> Result<&SubAgent, String> {
        self.spawn_scoped(
            id,
            name,
            "primary".to_string(),
            task_id.clone(),
            task_id,
            objective,
        )
    }

    /// Spawn in an explicit branch and mission scope.
    pub fn spawn_scoped(
        &mut self,
        id: String,
        name: String,
        branch_id: String,
        mission_id: String,
        task_id: String,
        objective: String,
    ) -> Result<&SubAgent, String> {
        if self.active_count() >= self.max_subagents {
            return Err(format!(
                "Cannot spawn agent: active count ({}) >= max ({})",
                self.active_count(),
                self.max_subagents
            ));
        }

        let branch_id = branch_id.trim().to_ascii_lowercase();
        let mission_id = mission_id.trim().to_string();
        if branch_id.is_empty() {
            return Err("Branch id cannot be empty".to_string());
        }
        if mission_id.is_empty() {
            return Err("Mission id cannot be empty".to_string());
        }

        self.ensure_branch(&branch_id)?;
        self.ensure_mission(&mission_id, &branch_id, &objective)?;

        let agent = SubAgent {
            id,
            name,
            branch_id,
            mission_id,
            task_id,
            objective,
            status: AgentStatus::Idle,
        };
        self.agents.push(agent);
        Ok(self.agents.last().unwrap())
    }

    /// Get all agents.
    pub fn list(&self) -> &[SubAgent] {
        &self.agents
    }

    pub fn get(&self, agent_id: &str) -> Option<&SubAgent> {
        self.agents.iter().find(|a| a.id == agent_id)
    }

    /// Rename an agent. Validates length, charset, uniqueness, and reserved names.
    pub fn rename(&mut self, agent_id: &str, new_name: &str) -> Result<(), String> {
        let trimmed = new_name.trim();

        if trimmed.is_empty() || trimmed.len() > 64 {
            return Err("Agent name must be 1-64 characters.".to_string());
        }

        if !trimmed
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == ' ' || c == '_')
        {
            return Err(
                "Agent name may only contain alphanumeric characters, hyphens, underscores, and spaces."
                    .to_string(),
            );
        }

        let reserved = ["kaizen", "system", "admin", "operator", "human"];
        if reserved.contains(&trimmed.to_lowercase().as_str()) {
            return Err(format!("'{}' is a reserved name.", trimmed));
        }

        let has_duplicate = self
            .agents
            .iter()
            .any(|a| a.id != agent_id && a.name.eq_ignore_ascii_case(trimmed));
        if has_duplicate {
            return Err(format!("Another agent already has the name '{}'.", trimmed));
        }

        let agent = self
            .agents
            .iter_mut()
            .find(|a| a.id == agent_id)
            .ok_or_else(|| format!("Agent not found: {agent_id}"))?;

        agent.name = trimmed.to_string();
        Ok(())
    }

    /// Remove an agent from the registry entirely.
    pub fn remove(&mut self, agent_id: &str) -> Result<SubAgent, String> {
        let idx = self
            .agents
            .iter()
            .position(|a| a.id == agent_id)
            .ok_or_else(|| format!("Agent not found: {agent_id}"))?;
        Ok(self.agents.remove(idx))
    }

    /// Transition an agent to a new status.
    ///
    /// Enforced rules (from policy):
    /// - idle -> active requires assignment
    /// - active -> review_pending requires deliverable
    /// - review_pending -> done requires Kaizen approval
    /// - review_pending -> active allowed for rework
    pub fn set_status(
        &mut self,
        agent_id: &str,
        status: AgentStatus,
        kaizen_review_approved: bool,
    ) -> Result<(), String> {
        let agent = self
            .agents
            .iter_mut()
            .find(|a| a.id == agent_id)
            .ok_or_else(|| format!("Agent not found: {agent_id}"))?;

        let from = agent.status;
        let to = status;

        let allowed = match (from, to) {
            (AgentStatus::Idle, AgentStatus::Active) => true,
            (AgentStatus::Idle, AgentStatus::Blocked) => true,
            (AgentStatus::Active, AgentStatus::ReviewPending) => true,
            (AgentStatus::ReviewPending, AgentStatus::Done) => kaizen_review_approved,
            (AgentStatus::ReviewPending, AgentStatus::Active) => true,
            (AgentStatus::ReviewPending, AgentStatus::Blocked) => true,
            (AgentStatus::Active, AgentStatus::Blocked) => true,
            (AgentStatus::Blocked, AgentStatus::Active) => true,
            // Idempotent state updates are allowed.
            (a, b) if a == b => true,
            _ => false,
        };

        if !allowed {
            return Err(format!(
                "Invalid lifecycle transition: {from:?} -> {to:?} (kaizen_review_approved={kaizen_review_approved})"
            ));
        }

        agent.status = status;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_within_limit() {
        let mut registry = AgentRegistry::new(2);
        let result = registry.spawn(
            "a1".into(),
            "TestAgent".into(),
            "t1".into(),
            "Do something".into(),
        );
        assert!(result.is_ok());
        assert_eq!(registry.active_count(), 1);
    }

    #[test]
    fn test_spawn_over_limit() {
        let mut registry = AgentRegistry::new(1);
        registry
            .spawn("a1".into(), "Agent1".into(), "t1".into(), "Task1".into())
            .unwrap();
        let result = registry.spawn("a2".into(), "Agent2".into(), "t2".into(), "Task2".into());
        assert!(result.is_err());
    }

    #[test]
    fn test_done_agents_dont_count_toward_limit() {
        let mut registry = AgentRegistry::new(1);
        registry
            .spawn("a1".into(), "Agent1".into(), "t1".into(), "Task1".into())
            .unwrap();
        registry
            .set_status("a1", AgentStatus::Active, false)
            .unwrap();
        registry
            .set_status("a1", AgentStatus::ReviewPending, false)
            .unwrap();
        registry.set_status("a1", AgentStatus::Done, true).unwrap();
        let result = registry.spawn("a2".into(), "Agent2".into(), "t2".into(), "Task2".into());
        assert!(result.is_ok());
    }

    #[test]
    fn test_cannot_finish_without_kaizen_approval() {
        let mut registry = AgentRegistry::new(1);
        registry
            .spawn("a1".into(), "Agent1".into(), "t1".into(), "Task1".into())
            .unwrap();
        registry
            .set_status("a1", AgentStatus::Active, false)
            .unwrap();
        registry
            .set_status("a1", AgentStatus::ReviewPending, false)
            .unwrap();

        let result = registry.set_status("a1", AgentStatus::Done, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_skip_transition_is_blocked() {
        let mut registry = AgentRegistry::new(1);
        registry
            .spawn("a1".into(), "Agent1".into(), "t1".into(), "Task1".into())
            .unwrap();

        let result = registry.set_status("a1", AgentStatus::Done, true);
        assert!(result.is_err());
    }

    #[test]
    fn test_idle_can_transition_to_blocked_for_operator_stop() {
        let mut registry = AgentRegistry::new(1);
        registry
            .spawn("a1".into(), "Agent1".into(), "t1".into(), "Task1".into())
            .unwrap();

        registry
            .set_status("a1", AgentStatus::Blocked, false)
            .unwrap();
        assert_eq!(registry.get("a1").unwrap().status, AgentStatus::Blocked);
    }

    #[test]
    fn test_review_pending_can_transition_to_blocked_for_operator_stop() {
        let mut registry = AgentRegistry::new(1);
        registry
            .spawn("a1".into(), "Agent1".into(), "t1".into(), "Task1".into())
            .unwrap();
        registry
            .set_status("a1", AgentStatus::Active, false)
            .unwrap();
        registry
            .set_status("a1", AgentStatus::ReviewPending, false)
            .unwrap();

        registry
            .set_status("a1", AgentStatus::Blocked, false)
            .unwrap();
        assert_eq!(registry.get("a1").unwrap().status, AgentStatus::Blocked);
    }

    #[test]
    fn test_rename_agent() {
        let mut registry = AgentRegistry::new(2);
        registry
            .spawn("a1".into(), "Agent1".into(), "t1".into(), "Task1".into())
            .unwrap();

        registry.rename("a1", "Builder-Alpha").unwrap();
        assert_eq!(registry.get("a1").unwrap().name, "Builder-Alpha");
    }

    #[test]
    fn test_rename_rejects_reserved_names() {
        let mut registry = AgentRegistry::new(2);
        registry
            .spawn("a1".into(), "Agent1".into(), "t1".into(), "Task1".into())
            .unwrap();

        let result = registry.rename("a1", "Kaizen");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("reserved"));
    }

    #[test]
    fn test_rename_rejects_duplicates() {
        let mut registry = AgentRegistry::new(3);
        registry
            .spawn("a1".into(), "Agent1".into(), "t1".into(), "Task1".into())
            .unwrap();
        registry
            .spawn("a2".into(), "Agent2".into(), "t2".into(), "Task2".into())
            .unwrap();

        let result = registry.rename("a1", "Agent2");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already has the name"));
    }

    #[test]
    fn test_rename_rejects_invalid_chars() {
        let mut registry = AgentRegistry::new(2);
        registry
            .spawn("a1".into(), "Agent1".into(), "t1".into(), "Task1".into())
            .unwrap();

        let result = registry.rename("a1", "Agent<script>");
        assert!(result.is_err());
    }

    #[test]
    fn test_rename_rejects_empty_or_long() {
        let mut registry = AgentRegistry::new(2);
        registry
            .spawn("a1".into(), "Agent1".into(), "t1".into(), "Task1".into())
            .unwrap();

        assert!(registry.rename("a1", "").is_err());
        assert!(registry.rename("a1", &"x".repeat(65)).is_err());
    }
}
