import type {
  Agent,
  AgentStatus,
  ArchiveIntegrityReport,
  ChatResponse,
  CrystalBallHealth,
  CrystalBallSmokeResponse,
  CrystalBallValidateResponse,
  CrystalBallEvent,
  GateConditionPatch,
  GateSnapshot,
  GateTransitionResult,
  KaizenSettings,
  KaizenSettingsPatch,
  SecretMetadata,
  SecretTestResult,
} from "./types";

interface ApiAgent {
  id: string;
  name: string;
  task_id: string;
  objective: string;
  status: AgentStatus;
}

interface SpawnAgentInput {
  agent_name: string;
  task_id: string;
  objective: string;
}

interface UpdateAgentStatusInput {
  status: AgentStatus;
  kaizen_review_approved?: boolean;
}

interface ApiCrystalBallEvent {
  event_id: string;
  timestamp: string;
  type: string;
  source_actor: string;
  source_agent_id: string;
  target_actor: string;
  target_agent_id: string;
  task_id: string;
  message: string;
  visibility: "operator" | "admin" | "audit";
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    headers: {
      "Content-Type": "application/json",
      ...(init?.headers ?? {}),
    },
    ...init,
  });

  if (!response.ok) {
    const errorText = await response.text();
    throw new Error(errorText || `Request failed: ${response.status}`);
  }

  return (await response.json()) as T;
}

function mapApiAgent(apiAgent: ApiAgent, defaultOpen = false): Agent {
  return {
    id: apiAgent.id,
    name: apiAgent.name,
    taskId: apiAgent.task_id,
    objective: apiAgent.objective,
    status: apiAgent.status,
    chatOpen: defaultOpen,
    messages: [],
  };
}

export async function fetchSettings(): Promise<KaizenSettings> {
  return request<KaizenSettings>("/api/settings");
}

export async function patchSettings(
  patch: KaizenSettingsPatch
): Promise<KaizenSettings> {
  return request<KaizenSettings>("/api/settings", {
    method: "PATCH",
    body: JSON.stringify(patch),
  });
}

export async function sendChat(
  message: string,
  agentId?: string
): Promise<ChatResponse> {
  return request<ChatResponse>("/api/chat", {
    method: "POST",
    body: JSON.stringify({
      message,
      agent_id: agentId,
    }),
  });
}

export async function fetchAgents(defaultOpen = false): Promise<Agent[]> {
  const apiAgents = await request<ApiAgent[]>("/api/agents");
  return apiAgents.map((agent) => mapApiAgent(agent, defaultOpen));
}

export async function createAgent(
  input: SpawnAgentInput,
  defaultOpen = false
): Promise<Agent> {
  const created = await request<ApiAgent>("/api/agents", {
    method: "POST",
    body: JSON.stringify({ ...input, user_requested: true }),
  });
  return mapApiAgent(created, defaultOpen);
}

export async function updateAgentStatus(
  agentId: string,
  input: UpdateAgentStatusInput
): Promise<Agent> {
  const updated = await request<ApiAgent>(`/api/agents/${agentId}/status`, {
    method: "PATCH",
    body: JSON.stringify(input),
  });
  return mapApiAgent(updated, false);
}

export async function fetchGateSnapshot(): Promise<GateSnapshot> {
  return request<GateSnapshot>("/api/gates");
}

export async function patchGateConditions(
  patch: GateConditionPatch
): Promise<GateSnapshot> {
  return request<GateSnapshot>("/api/gates/conditions", {
    method: "PATCH",
    body: JSON.stringify(patch),
  });
}

export async function advanceGate(): Promise<GateTransitionResult> {
  return request<GateTransitionResult>("/api/gates/advance", {
    method: "POST",
  });
}

export async function fetchCrystalBallEvents(
  limit = 100
): Promise<CrystalBallEvent[]> {
  const events = await request<ApiCrystalBallEvent[]>(`/api/events?limit=${limit}`);
  return events.map((event) => ({
    eventId: event.event_id,
    timestamp: event.timestamp,
    type: event.type,
    sourceActor: event.source_actor,
    sourceAgentId: event.source_agent_id,
    targetActor: event.target_actor,
    targetAgentId: event.target_agent_id,
    taskId: event.task_id,
    message: event.message,
    visibility: event.visibility,
  }));
}

export async function fetchCrystalBallHealth(): Promise<CrystalBallHealth> {
  return request<CrystalBallHealth>("/api/crystal-ball/health");
}

export async function fetchCrystalBallAudit(): Promise<ArchiveIntegrityReport> {
  return request<ArchiveIntegrityReport>("/api/crystal-ball/audit");
}

export async function validateCrystalBall(): Promise<CrystalBallValidateResponse> {
  return request<CrystalBallValidateResponse>("/api/crystal-ball/validate");
}

export async function runCrystalBallSmoke(): Promise<CrystalBallSmokeResponse> {
  return request<CrystalBallSmokeResponse>("/api/crystal-ball/smoke", {
    method: "POST",
  });
}

// ---- Agent Rename ----

export async function renameAgent(
  agentId: string,
  name: string
): Promise<Agent> {
  const updated = await request<ApiAgent>(`/api/agents/${agentId}`, {
    method: "PATCH",
    body: JSON.stringify({ name }),
  });
  return mapApiAgent(updated, false);
}

// ---- Secret Vault ----

export async function fetchSecrets(): Promise<SecretMetadata[]> {
  return request<SecretMetadata[]>("/api/secrets");
}

export async function storeSecret(
  provider: string,
  value: string,
  secretType = "api_key"
): Promise<SecretMetadata> {
  return request<SecretMetadata>(`/api/secrets/${provider}`, {
    method: "PUT",
    body: JSON.stringify({ value, secret_type: secretType }),
  });
}

export async function revokeSecret(provider: string): Promise<void> {
  await fetch(`/api/secrets/${provider}`, { method: "DELETE" });
}

export async function testSecret(
  provider: string
): Promise<SecretTestResult> {
  return request<SecretTestResult>(`/api/secrets/${provider}/test`, {
    method: "POST",
  });
}

// ---- OAuth ----

export async function oauthDisconnect(provider: string): Promise<void> {
  await fetch(`/api/oauth/${provider}`, { method: "DELETE" });
}
