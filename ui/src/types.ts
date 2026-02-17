/** Agent lifecycle status - mirrors core/src/agents.rs */
export type AgentStatus =
  | "idle"
  | "active"
  | "blocked"
  | "review_pending"
  | "done";

/** Sub-agent representation */
export interface Agent {
  id: string;
  name: string;
  taskId: string;
  objective: string;
  status: AgentStatus;
  chatOpen: boolean;
}

/** Gate states - mirrors core/src/gate_engine.rs */
export type GateState =
  | "plan"
  | "execute"
  | "review"
  | "human_smoke_test"
  | "deploy"
  | "complete";

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
}
