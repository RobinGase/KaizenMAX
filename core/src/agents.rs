//! Agent lifecycle management
//!
//! Handles sub-agent spawning, status tracking, and lifecycle transitions
//! per the agent_control_policy.yaml.

use serde::{Deserialize, Serialize};

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
    pub task_id: String,
    pub objective: String,
    pub status: AgentStatus,
}

/// Agent registry that enforces spawn limits and lifecycle rules.
#[derive(Debug)]
pub struct AgentRegistry {
    agents: Vec<SubAgent>,
    max_subagents: usize,
}

impl AgentRegistry {
    pub fn new(max_subagents: usize) -> Self {
        Self {
            agents: Vec::new(),
            max_subagents,
        }
    }

    pub fn set_max_subagents(&mut self, max_subagents: usize) {
        self.max_subagents = max_subagents;
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
        if self.active_count() >= self.max_subagents {
            return Err(format!(
                "Cannot spawn agent: active count ({}) >= max ({})",
                self.active_count(),
                self.max_subagents
            ));
        }

        let agent = SubAgent {
            id,
            name,
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
            (AgentStatus::Active, AgentStatus::ReviewPending) => true,
            (AgentStatus::ReviewPending, AgentStatus::Done) => kaizen_review_approved,
            (AgentStatus::ReviewPending, AgentStatus::Active) => true,
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
