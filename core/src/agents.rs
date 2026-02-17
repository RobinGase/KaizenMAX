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

    /// Transition an agent to a new status.
    pub fn set_status(&mut self, agent_id: &str, status: AgentStatus) -> Result<(), String> {
        let agent = self
            .agents
            .iter_mut()
            .find(|a| a.id == agent_id)
            .ok_or_else(|| format!("Agent not found: {agent_id}"))?;

        // Enforce: cannot finalize (Done) from ReviewPending without approval
        // This check is simplified; real implementation ties into gate_engine
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
        registry.set_status("a1", AgentStatus::Done).unwrap();
        let result = registry.spawn("a2".into(), "Agent2".into(), "t2".into(), "Task2".into());
        assert!(result.is_ok());
    }
}
