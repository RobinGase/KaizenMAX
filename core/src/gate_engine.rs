//! Hard Gate Engine
//!
//! Implements the enforced state machine from Section 8 of the implementation plan:
//! Plan -> Execute -> Review -> Human Smoke Test -> Deploy -> Complete
//!
//! Hard-gate rules:
//! - No agent may finalize output without Kaizen approval.
//! - Required review checkpoint: Passed Reasoners Test.
//! - If review fails, flow returns to Execute/Review until passed.

use serde::{Deserialize, Serialize};

/// States in the orchestration pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateState {
    Plan,
    Execute,
    Review,
    HumanSmokeTest,
    Deploy,
    Complete,
}

/// Conditions that must be satisfied for gate transitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateConditions {
    pub plan_defined: bool,
    pub plan_acknowledged: bool,
    pub execution_artifacts_present: bool,
    pub passed_reasoners_test: bool,
    pub kaizen_review_approved: bool,
    pub human_smoke_test_passed: bool,
    pub deploy_validation_passed: bool,
}

impl Default for GateConditions {
    fn default() -> Self {
        Self {
            plan_defined: false,
            plan_acknowledged: false,
            execution_artifacts_present: false,
            passed_reasoners_test: false,
            kaizen_review_approved: false,
            human_smoke_test_passed: false,
            deploy_validation_passed: false,
        }
    }
}

/// Result of attempting a gate transition.
#[derive(Debug, Serialize)]
pub struct TransitionResult {
    pub allowed: bool,
    pub from: GateState,
    pub to: GateState,
    pub blocked_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateRuntime {
    pub current_state: GateState,
    pub conditions: GateConditions,
}

impl Default for GateRuntime {
    fn default() -> Self {
        Self {
            current_state: GateState::Plan,
            conditions: GateConditions::default(),
        }
    }
}

impl GateRuntime {
    pub fn update_conditions(&mut self, patch: GateConditionPatch) {
        if let Some(value) = patch.plan_defined {
            self.conditions.plan_defined = value;
        }
        if let Some(value) = patch.plan_acknowledged {
            self.conditions.plan_acknowledged = value;
        }
        if let Some(value) = patch.execution_artifacts_present {
            self.conditions.execution_artifacts_present = value;
        }
        if let Some(value) = patch.passed_reasoners_test {
            self.conditions.passed_reasoners_test = value;
        }
        if let Some(value) = patch.kaizen_review_approved {
            self.conditions.kaizen_review_approved = value;
        }
        if let Some(value) = patch.human_smoke_test_passed {
            self.conditions.human_smoke_test_passed = value;
        }
        if let Some(value) = patch.deploy_validation_passed {
            self.conditions.deploy_validation_passed = value;
        }
    }

    pub fn advance(&mut self) -> TransitionResult {
        let result = try_transition(self.current_state, &self.conditions);
        if result.allowed {
            self.current_state = result.to;
        }
        result
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct GateConditionPatch {
    pub plan_defined: Option<bool>,
    pub plan_acknowledged: Option<bool>,
    pub execution_artifacts_present: Option<bool>,
    pub passed_reasoners_test: Option<bool>,
    pub kaizen_review_approved: Option<bool>,
    pub human_smoke_test_passed: Option<bool>,
    pub deploy_validation_passed: Option<bool>,
}

/// Attempt to transition from current state to the next.
pub fn try_transition(current: GateState, conditions: &GateConditions) -> TransitionResult {
    let (next, missing) = match current {
        GateState::Plan => {
            let mut blocked = vec![];
            if !conditions.plan_defined {
                blocked.push("plan_defined".to_string());
            }
            if !conditions.plan_acknowledged {
                blocked.push("plan_acknowledged".to_string());
            }
            (GateState::Execute, blocked)
        }
        GateState::Execute => {
            let mut blocked = vec![];
            if !conditions.execution_artifacts_present {
                blocked.push("execution_artifacts_present".to_string());
            }
            (GateState::Review, blocked)
        }
        GateState::Review => {
            let mut blocked = vec![];
            if !conditions.passed_reasoners_test {
                blocked.push("passed_reasoners_test".to_string());
            }
            if !conditions.kaizen_review_approved {
                blocked.push("kaizen_review_approved".to_string());
            }
            (GateState::HumanSmokeTest, blocked)
        }
        GateState::HumanSmokeTest => {
            let mut blocked = vec![];
            if !conditions.human_smoke_test_passed {
                blocked.push("human_smoke_test_passed".to_string());
            }
            (GateState::Deploy, blocked)
        }
        GateState::Deploy => {
            let mut blocked = vec![];
            if !conditions.deploy_validation_passed {
                blocked.push("deploy_validation_passed".to_string());
            }
            (GateState::Complete, blocked)
        }
        GateState::Complete => {
            // Terminal state - no further transitions
            return TransitionResult {
                allowed: false,
                from: current,
                to: GateState::Complete,
                blocked_by: vec!["already_complete".to_string()],
            };
        }
    };

    TransitionResult {
        allowed: missing.is_empty(),
        from: current,
        to: next,
        blocked_by: missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_to_execute_blocked_without_conditions() {
        let conditions = GateConditions::default();
        let result = try_transition(GateState::Plan, &conditions);
        assert!(!result.allowed);
        assert_eq!(result.blocked_by.len(), 2);
    }

    #[test]
    fn test_plan_to_execute_allowed() {
        let conditions = GateConditions {
            plan_defined: true,
            plan_acknowledged: true,
            ..Default::default()
        };
        let result = try_transition(GateState::Plan, &conditions);
        assert!(result.allowed);
        assert_eq!(result.to, GateState::Execute);
    }

    #[test]
    fn test_review_blocked_without_reasoners_test() {
        let conditions = GateConditions {
            kaizen_review_approved: true,
            ..Default::default()
        };
        let result = try_transition(GateState::Review, &conditions);
        assert!(!result.allowed);
        assert!(
            result
                .blocked_by
                .contains(&"passed_reasoners_test".to_string())
        );
    }

    #[test]
    fn test_deploy_blocked_without_smoke_test() {
        let conditions = GateConditions::default();
        let result = try_transition(GateState::HumanSmokeTest, &conditions);
        assert!(!result.allowed);
        assert!(
            result
                .blocked_by
                .contains(&"human_smoke_test_passed".to_string())
        );
    }

    #[test]
    fn test_runtime_advance_updates_state() {
        let mut runtime = GateRuntime::default();
        runtime.update_conditions(GateConditionPatch {
            plan_defined: Some(true),
            plan_acknowledged: Some(true),
            execution_artifacts_present: None,
            passed_reasoners_test: None,
            kaizen_review_approved: None,
            human_smoke_test_passed: None,
            deploy_validation_passed: None,
        });

        let result = runtime.advance();
        assert!(result.allowed);
        assert_eq!(runtime.current_state, GateState::Execute);
    }
}
