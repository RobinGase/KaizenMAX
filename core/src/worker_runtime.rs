//! Background worker queue and heartbeat state.
//!
//! This module persists delegated work items and short-lived worker heartbeats
//! so the gateway can execute staff jobs outside the request/response path.

use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerJobStatus {
    Pending,
    Claimed,
    Running,
    Completed,
    Blocked,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerToolStepStatus {
    Running,
    Completed,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerToolStep {
    pub tool_step_id: String,
    pub job_id: String,
    pub tool_id: String,
    pub action: String,
    pub status: WorkerToolStepStatus,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    pub input_summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_summary: Option<String>,
    #[serde(default)]
    pub artifact_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerJob {
    pub job_id: String,
    pub agent_id: String,
    pub branch_id: String,
    pub mission_id: String,
    pub task_id: String,
    pub instruction: String,
    pub requested_by: String,
    pub source_conversation: String,
    pub status: WorkerJobStatus,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default)]
    pub attempt_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_instance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_step: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub artifact_paths: Vec<String>,
    #[serde(default)]
    pub tool_steps: Vec<WorkerToolStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerHeartbeat {
    pub agent_id: String,
    pub job_id: String,
    pub worker_instance_id: String,
    pub status: WorkerJobStatus,
    pub current_step: String,
    pub progress_message: String,
    pub last_heartbeat_at: String,
    pub heartbeat_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerJobLease {
    pub job_id: String,
    pub agent_id: String,
    pub worker_instance_id: String,
}

#[derive(Debug, Default)]
pub struct WorkerRuntimeState {
    jobs: Vec<WorkerJob>,
    heartbeats: HashMap<String, WorkerHeartbeat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct WorkerRuntimeSnapshot {
    #[serde(default)]
    jobs: Vec<WorkerJob>,
    #[serde(default)]
    heartbeats: Vec<WorkerHeartbeat>,
}

impl WorkerRuntimeState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load_from_path(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::new());
        }

        let text = std::fs::read_to_string(path)
            .map_err(|err| format!("Failed to read worker runtime {}: {err}", path.display()))?;
        let snapshot: WorkerRuntimeSnapshot = serde_json::from_str(&text).map_err(|err| {
            format!(
                "Failed to parse worker runtime snapshot {}: {err}",
                path.display()
            )
        })?;

        let mut state = Self {
            jobs: snapshot.jobs,
            heartbeats: snapshot
                .heartbeats
                .into_iter()
                .map(|heartbeat| (heartbeat.agent_id.clone(), heartbeat))
                .collect(),
        };
        state.recover_inflight();
        Ok(state)
    }

    pub fn persist_to_path(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|err| {
                    format!(
                        "Failed to create worker runtime directory {}: {err}",
                        parent.display()
                    )
                })?;
            }
        }

        let snapshot = WorkerRuntimeSnapshot {
            jobs: self.jobs.clone(),
            heartbeats: self.heartbeats.values().cloned().collect(),
        };

        let json = serde_json::to_string_pretty(&snapshot)
            .map_err(|err| format!("Failed to serialize worker runtime: {err}"))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|err| {
            format!(
                "Failed to write worker runtime tmp {}: {err}",
                tmp.display()
            )
        })?;
        std::fs::rename(&tmp, path)
            .map_err(|err| format!("Failed to persist worker runtime {}: {err}", path.display()))?;
        Ok(())
    }

    pub fn recover_inflight(&mut self) {
        for job in &mut self.jobs {
            if matches!(
                job.status,
                WorkerJobStatus::Claimed | WorkerJobStatus::Running
            ) {
                job.status = WorkerJobStatus::Pending;
                job.worker_instance_id = None;
                job.current_step = Some("recovered".to_string());
                job.progress_message = Some(
                    "Recovered after runtime restart; waiting to be picked up again.".to_string(),
                );
                job.started_at = None;
            }
        }
        self.heartbeats.clear();
    }

    pub fn enqueue_job(
        &mut self,
        job_id: String,
        agent_id: String,
        branch_id: String,
        mission_id: String,
        task_id: String,
        instruction: String,
        requested_by: String,
        source_conversation: String,
        now: String,
    ) -> WorkerJob {
        let job = WorkerJob {
            job_id,
            agent_id,
            branch_id,
            mission_id,
            task_id,
            instruction,
            requested_by,
            source_conversation,
            status: WorkerJobStatus::Pending,
            created_at: now.clone(),
            updated_at: now,
            started_at: None,
            finished_at: None,
            attempt_count: 0,
            worker_instance_id: None,
            current_step: Some("queued".to_string()),
            progress_message: Some("Queued for background execution.".to_string()),
            result: None,
            error: None,
            artifact_paths: Vec::new(),
            tool_steps: Vec::new(),
        };
        self.jobs.push(job.clone());
        job
    }

    pub fn list_jobs(&self) -> &[WorkerJob] {
        &self.jobs
    }

    pub fn list_recent_jobs(&self, limit: usize) -> Vec<WorkerJob> {
        let mut jobs = self.jobs.clone();
        jobs.sort_by(|left, right| left.updated_at.cmp(&right.updated_at));
        jobs.reverse();
        jobs.into_iter().take(limit).collect()
    }

    pub fn list_heartbeats(&self) -> Vec<WorkerHeartbeat> {
        self.heartbeats.values().cloned().collect()
    }

    pub fn active_heartbeat_for_agent(
        &self,
        agent_id: &str,
        now_ts: f64,
        stale_after_secs: f64,
    ) -> Option<&WorkerHeartbeat> {
        let heartbeat = self.heartbeats.get(agent_id)?;
        let last = parse_timestamp_seconds(&heartbeat.last_heartbeat_at)?;
        if now_ts - last <= stale_after_secs {
            Some(heartbeat)
        } else {
            None
        }
    }

    pub fn reclaim_stale_jobs(
        &mut self,
        now_ts: f64,
        stale_after_secs: f64,
        now: &str,
    ) -> Vec<WorkerJob> {
        let mut stale_agents = Vec::new();
        for (agent_id, heartbeat) in &self.heartbeats {
            let Some(last) = parse_timestamp_seconds(&heartbeat.last_heartbeat_at) else {
                stale_agents.push(agent_id.clone());
                continue;
            };
            if now_ts - last > stale_after_secs {
                stale_agents.push(agent_id.clone());
            }
        }

        if stale_agents.is_empty() {
            return Vec::new();
        }

        let mut reclaimed = Vec::new();
        for agent_id in stale_agents {
            let Some(heartbeat) = self.heartbeats.remove(&agent_id) else {
                continue;
            };
            if let Some(job) = self.jobs.iter_mut().find(|job| {
                job.job_id == heartbeat.job_id
                    && matches!(
                        job.status,
                        WorkerJobStatus::Claimed | WorkerJobStatus::Running
                    )
            }) {
                job.status = WorkerJobStatus::Pending;
                job.worker_instance_id = None;
                job.current_step = Some("reclaimed".to_string());
                job.progress_message =
                    Some("Worker heartbeat went stale; job returned to queue.".to_string());
                job.updated_at = now.to_string();
                reclaimed.push(job.clone());
            }
        }

        reclaimed
    }

    pub fn claim_pending_jobs(
        &mut self,
        max_jobs: usize,
        now_ts: f64,
        stale_after_secs: f64,
        now: &str,
    ) -> Vec<WorkerJobLease> {
        let mut busy_agents = HashMap::new();
        for job in &self.jobs {
            if matches!(
                job.status,
                WorkerJobStatus::Claimed | WorkerJobStatus::Running
            ) {
                busy_agents.insert(job.agent_id.clone(), true);
            }
        }

        for agent in self.heartbeats.keys().filter(|agent_id| {
            self.active_heartbeat_for_agent(agent_id, now_ts, stale_after_secs)
                .is_some()
        }) {
            busy_agents.insert(agent.clone(), true);
        }

        let mut leases = Vec::new();
        for job in &mut self.jobs {
            if leases.len() >= max_jobs {
                break;
            }
            if job.status != WorkerJobStatus::Pending {
                continue;
            }
            if busy_agents.contains_key(&job.agent_id) {
                continue;
            }

            let worker_instance_id = Uuid::new_v4().to_string();
            job.status = WorkerJobStatus::Claimed;
            job.attempt_count += 1;
            job.started_at = Some(now.to_string());
            job.updated_at = now.to_string();
            job.worker_instance_id = Some(worker_instance_id.clone());
            job.current_step = Some("claimed".to_string());
            job.progress_message = Some("Worker claimed the job.".to_string());
            job.error = None;

            self.heartbeats.insert(
                job.agent_id.clone(),
                WorkerHeartbeat {
                    agent_id: job.agent_id.clone(),
                    job_id: job.job_id.clone(),
                    worker_instance_id: worker_instance_id.clone(),
                    status: WorkerJobStatus::Claimed,
                    current_step: "claimed".to_string(),
                    progress_message: "Worker claimed the job.".to_string(),
                    last_heartbeat_at: now.to_string(),
                    heartbeat_seq: 1,
                    current_tool: None,
                    current_action: None,
                },
            );

            busy_agents.insert(job.agent_id.clone(), true);
            leases.push(WorkerJobLease {
                job_id: job.job_id.clone(),
                agent_id: job.agent_id.clone(),
                worker_instance_id,
            });
        }

        leases
    }

    pub fn start_job(
        &mut self,
        job_id: &str,
        worker_instance_id: &str,
        now: &str,
        step: &str,
        message: &str,
    ) {
        if let Some(job) = self.jobs.iter_mut().find(|job| job.job_id == job_id) {
            job.status = WorkerJobStatus::Running;
            job.updated_at = now.to_string();
            job.worker_instance_id = Some(worker_instance_id.to_string());
            job.current_step = Some(step.to_string());
            job.progress_message = Some(message.to_string());
        }
        self.touch_heartbeat_for_job(
            job_id,
            worker_instance_id,
            WorkerJobStatus::Running,
            step,
            message,
            None,
            None,
            now,
        );
    }

    pub fn heartbeat(
        &mut self,
        job_id: &str,
        worker_instance_id: &str,
        status: WorkerJobStatus,
        step: &str,
        message: &str,
        current_tool: Option<&str>,
        current_action: Option<&str>,
        now: &str,
    ) {
        if let Some(job) = self.jobs.iter_mut().find(|job| {
            job.job_id == job_id && job.worker_instance_id.as_deref() == Some(worker_instance_id)
        }) {
            job.status = status;
            job.updated_at = now.to_string();
            job.current_step = Some(step.to_string());
            job.progress_message = Some(message.to_string());
        }
        self.touch_heartbeat_for_job(
            job_id,
            worker_instance_id,
            status,
            step,
            message,
            current_tool,
            current_action,
            now,
        );
    }

    pub fn begin_tool_step(
        &mut self,
        job_id: &str,
        tool_id: &str,
        action: &str,
        input_summary: &str,
        now: &str,
    ) -> Option<WorkerToolStep> {
        let job = self.jobs.iter_mut().find(|job| job.job_id == job_id)?;
        let step = WorkerToolStep {
            tool_step_id: Uuid::new_v4().to_string(),
            job_id: job_id.to_string(),
            tool_id: tool_id.to_string(),
            action: action.to_string(),
            status: WorkerToolStepStatus::Running,
            started_at: now.to_string(),
            finished_at: None,
            input_summary: input_summary.to_string(),
            output_summary: None,
            artifact_paths: Vec::new(),
            error: None,
        };
        job.tool_steps.push(step.clone());
        job.current_step = Some(format!("tool:{}:{}", tool_id, action));
        job.progress_message = Some(format!("Running {} {}.", tool_id, action));
        Some(step)
    }

    pub fn finish_tool_step(
        &mut self,
        job_id: &str,
        tool_step_id: &str,
        status: WorkerToolStepStatus,
        output_summary: Option<String>,
        artifact_paths: Vec<String>,
        error: Option<String>,
        now: &str,
    ) {
        if let Some(job) = self.jobs.iter_mut().find(|job| job.job_id == job_id) {
            if let Some(step) = job
                .tool_steps
                .iter_mut()
                .find(|step| step.tool_step_id == tool_step_id)
            {
                step.status = status;
                step.finished_at = Some(now.to_string());
                step.output_summary = output_summary.clone();
                step.artifact_paths = artifact_paths.clone();
                step.error = error.clone();
            }
            for artifact in artifact_paths {
                if !job
                    .artifact_paths
                    .iter()
                    .any(|existing| existing == &artifact)
                {
                    job.artifact_paths.push(artifact);
                }
            }
            job.updated_at = now.to_string();
        }
    }

    pub fn record_artifacts(&mut self, job_id: &str, artifact_paths: &[String], now: &str) {
        if let Some(job) = self.jobs.iter_mut().find(|job| job.job_id == job_id) {
            for artifact in artifact_paths {
                if !job
                    .artifact_paths
                    .iter()
                    .any(|existing| existing == artifact)
                {
                    job.artifact_paths.push(artifact.clone());
                }
            }
            job.updated_at = now.to_string();
        }
    }

    pub fn complete_job(
        &mut self,
        job_id: &str,
        worker_instance_id: &str,
        now: &str,
        result: String,
    ) -> Option<WorkerJob> {
        let mut completed = None;
        if let Some(job) = self.jobs.iter_mut().find(|job| {
            job.job_id == job_id && job.worker_instance_id.as_deref() == Some(worker_instance_id)
        }) {
            job.status = WorkerJobStatus::Completed;
            job.updated_at = now.to_string();
            job.finished_at = Some(now.to_string());
            job.current_step = Some("completed".to_string());
            job.progress_message = Some("Worker completed the assignment.".to_string());
            job.result = Some(result);
            completed = Some(job.clone());
        }
        self.remove_heartbeat_for_job(job_id, worker_instance_id);
        completed
    }

    pub fn block_job(
        &mut self,
        job_id: &str,
        worker_instance_id: &str,
        now: &str,
        error: String,
    ) -> Option<WorkerJob> {
        let mut blocked = None;
        if let Some(job) = self.jobs.iter_mut().find(|job| {
            job.job_id == job_id && job.worker_instance_id.as_deref() == Some(worker_instance_id)
        }) {
            job.status = WorkerJobStatus::Blocked;
            job.updated_at = now.to_string();
            job.finished_at = Some(now.to_string());
            job.current_step = Some("blocked".to_string());
            job.progress_message = Some(error.clone());
            job.error = Some(error);
            blocked = Some(job.clone());
        }
        self.remove_heartbeat_for_job(job_id, worker_instance_id);
        blocked
    }

    pub fn fail_job(
        &mut self,
        job_id: &str,
        worker_instance_id: &str,
        now: &str,
        error: String,
    ) -> Option<WorkerJob> {
        let mut failed = None;
        if let Some(job) = self.jobs.iter_mut().find(|job| {
            job.job_id == job_id && job.worker_instance_id.as_deref() == Some(worker_instance_id)
        }) {
            job.status = WorkerJobStatus::Failed;
            job.updated_at = now.to_string();
            job.finished_at = Some(now.to_string());
            job.current_step = Some("failed".to_string());
            job.progress_message = Some(error.clone());
            job.error = Some(error);
            failed = Some(job.clone());
        }
        self.remove_heartbeat_for_job(job_id, worker_instance_id);
        failed
    }

    pub fn latest_job_for_agent(&self, agent_id: &str) -> Option<WorkerJob> {
        self.jobs
            .iter()
            .filter(|job| job.agent_id == agent_id)
            .max_by(|left, right| left.updated_at.cmp(&right.updated_at))
            .cloned()
    }

    pub fn get_job(&self, job_id: &str) -> Option<WorkerJob> {
        self.jobs.iter().find(|job| job.job_id == job_id).cloned()
    }

    fn touch_heartbeat_for_job(
        &mut self,
        job_id: &str,
        worker_instance_id: &str,
        status: WorkerJobStatus,
        step: &str,
        message: &str,
        current_tool: Option<&str>,
        current_action: Option<&str>,
        now: &str,
    ) {
        let Some(agent_id) = self
            .jobs
            .iter()
            .find(|job| job.job_id == job_id)
            .map(|job| job.agent_id.clone())
        else {
            return;
        };

        let seq = self
            .heartbeats
            .get(&agent_id)
            .map(|heartbeat| heartbeat.heartbeat_seq + 1)
            .unwrap_or(1);

        self.heartbeats.insert(
            agent_id.clone(),
            WorkerHeartbeat {
                agent_id,
                job_id: job_id.to_string(),
                worker_instance_id: worker_instance_id.to_string(),
                status,
                current_step: step.to_string(),
                progress_message: message.to_string(),
                last_heartbeat_at: now.to_string(),
                heartbeat_seq: seq,
                current_tool: current_tool.map(|value| value.to_string()),
                current_action: current_action.map(|value| value.to_string()),
            },
        );
    }

    fn remove_heartbeat_for_job(&mut self, job_id: &str, worker_instance_id: &str) {
        let stale_agents = self
            .heartbeats
            .iter()
            .filter(|(_, heartbeat)| {
                heartbeat.job_id == job_id && heartbeat.worker_instance_id == worker_instance_id
            })
            .map(|(agent_id, _)| agent_id.clone())
            .collect::<Vec<_>>();

        for agent_id in stale_agents {
            self.heartbeats.remove(&agent_id);
        }
    }
}

fn parse_timestamp_seconds(value: &str) -> Option<f64> {
    value.parse::<f64>().ok()
}
