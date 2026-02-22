import {
  For,
  Show,
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
  onMount
} from "solid-js";
import { createStore } from "solid-js/store";
import { coreRequest } from "./lib/tauri";
import type {
  AgentStatus,
  ChatHistoryResponse,
  ChatMessage,
  ChatResponse,
  CrystalBallAudit,
  CrystalBallEvent,
  CrystalBallHealth,
  CrystalBallSmoke,
  CrystalBallValidation,
  GateConditions,
  GateSnapshot,
  GateState,
  GitHubReposResponse,
  GitHubStatus,
  HealthResponse,
  KaizenSettings,
  Notice,
  OAuthStatus,
  SecretMetadata,
  SubAgent,
  TransitionResult,
  VaultStatus
} from "./lib/types";

type TabId = "mission" | "gates" | "activity" | "workspace" | "integrations" | "settings";

interface WorkspaceTile {
  id: string;
  path: string;
}

const TABS: Array<{ id: TabId; label: string }> = [
  { id: "mission", label: "Mission" },
  { id: "gates", label: "Workflow Gates" },
  { id: "activity", label: "Activity" },
  { id: "workspace", label: "Workspace" },
  { id: "integrations", label: "Providers & Secrets" },
  { id: "settings", label: "System Settings" }
];

const KAIZEN_MODES = ["yolo", "build", "plan", "reason", "orchestrator"];
const SUBAGENT_MODES = ["build", "plan"];
const AGENT_STATUSES: AgentStatus[] = ["idle", "active", "blocked", "review_pending", "done"];
const SECRET_PROVIDERS = ["anthropic", "openai", "gemini", "nvidia", "kaizen"];
const OAUTH_PROVIDERS = ["openai", "anthropic"];
const PROVIDER_MODEL_HINTS: Record<string, string[]> = {
  anthropic: ["claude-sonnet-4-20250514", "claude-3-7-sonnet-latest"],
  openai: ["gpt-4.1", "gpt-4.1-mini", "o3-mini"],
  gemini: ["gemini-2.5-flash", "gemini-2.5-pro"],
  nvidia: ["meta/llama-3.1-70b-instruct", "mistralai/mixtral-8x7b-instruct-v0.1"],
  kaizen: ["use-configured-provider"]
};

const GATE_LABELS: Record<keyof GateConditions, string> = {
  plan_defined: "Plan defined",
  plan_acknowledged: "Plan acknowledged",
  execution_artifacts_present: "Execution artifacts present",
  passed_reasoners_test: "Reasoners test passed",
  kaizen_review_approved: "Kaizen review approved",
  human_smoke_test_passed: "Human smoke test passed",
  deploy_validation_passed: "Deploy validation passed"
};

function uid(prefix: string): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return `${prefix}-${crypto.randomUUID()}`;
  }
  return `${prefix}-${Date.now()}-${Math.floor(Math.random() * 10000)}`;
}

function parseError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === "string") {
    return error;
  }
  return "Unexpected error";
}

function loadWorkspaceTiles(): WorkspaceTile[] {
  const raw = localStorage.getItem("kaizen.workspace.tiles");
  if (!raw) {
    return [];
  }
  try {
    const parsed = JSON.parse(raw) as WorkspaceTile[];
    if (!Array.isArray(parsed)) {
      return [];
    }
    return parsed.filter((tile) => typeof tile.path === "string" && tile.path.trim().length > 0);
  } catch {
    return [];
  }
}

export default function App() {
  const [activeTab, setActiveTab] = createSignal<TabId>("mission");
  const [adminToken, setAdminToken] = createSignal(localStorage.getItem("kaizen.admin.token") ?? "");
  const [notices, setNotices] = createSignal<Notice[]>([]);

  const [busy, setBusy] = createStore<Record<string, boolean>>({});

  const [state, setState] = createStore({
    health: null as HealthResponse | null,
    settings: null as KaizenSettings | null,
    settingsDraft: {} as Partial<KaizenSettings>,
    agents: [] as SubAgent[],
    gates: null as GateSnapshot | null,
    gateDraft: null as GateConditions | null,
    gateTransition: null as TransitionResult | null,
    events: [] as CrystalBallEvent[],
    crystalHealth: null as CrystalBallHealth | null,
    crystalValidation: null as CrystalBallValidation | null,
    crystalSmoke: null as CrystalBallSmoke | null,
    crystalAudit: null as CrystalBallAudit | null,
    githubStatus: null as GitHubStatus | null,
    githubRepos: [] as GitHubReposResponse["repos"],
    selectedRepo: "",
    workspaceInput: "",
    workspaceTiles: loadWorkspaceTiles() as WorkspaceTile[],
    vaultStatus: null as VaultStatus | null,
    secrets: [] as SecretMetadata[],
    oauth: {} as Record<string, OAuthStatus | null>,
    revealedSecret: "",
    secretProvider: "anthropic",
    secretValue: "",
    secretType: "api_key",
    selectedAgentId: "",
    chatHistory: [] as ChatMessage[],
    chatMessage: "",
    chatMode: "yolo",
    chatProvider: "",
    chatModel: "",
    wrapMode: false,
    wrapTargets: "",
    clearHistory: false,
    lastChatMeta: "",
    newAgentName: "",
    newAgentTaskId: "",
    newAgentObjective: "",
    eventsLimit: 100
  });

  function pushNotice(kind: Notice["kind"], text: string): void {
    const id = uid("notice");
    setNotices((items) => [...items, { id, kind, text }]);
    setTimeout(() => {
      setNotices((items) => items.filter((entry) => entry.id !== id));
    }, kind === "error" ? 6500 : 3500);
  }

  async function runTask<T>(key: string, work: () => Promise<T>): Promise<T | null> {
    setBusy(key, true);
    try {
      return await work();
    } catch (error) {
      pushNotice("error", parseError(error));
      return null;
    } finally {
      setBusy(key, false);
    }
  }

  async function apiGet<T>(path: string): Promise<T> {
    return coreRequest<T>({ method: "GET", path, adminToken: adminToken() });
  }

  async function apiPost<T>(path: string, body?: unknown): Promise<T> {
    return coreRequest<T>({ method: "POST", path, body, adminToken: adminToken() });
  }

  async function apiPatch<T>(path: string, body?: unknown): Promise<T> {
    return coreRequest<T>({ method: "PATCH", path, body, adminToken: adminToken() });
  }

  async function apiPut<T>(path: string, body?: unknown): Promise<T> {
    return coreRequest<T>({ method: "PUT", path, body, adminToken: adminToken() });
  }

  async function apiDelete(path: string): Promise<void> {
    await coreRequest<unknown>({ method: "DELETE", path, adminToken: adminToken() });
  }

  async function refreshHealth(): Promise<void> {
    const payload = await apiGet<HealthResponse>("/health");
    setState("health", payload);
  }

  async function refreshSettings(): Promise<void> {
    const payload = await apiGet<KaizenSettings>("/api/settings");
    setState("settings", payload);
    setState("settingsDraft", { ...payload });
    if (!state.chatProvider) {
      setState("chatProvider", payload.inference_provider || "gemini");
    }
    if (!state.chatModel) {
      setState("chatModel", payload.inference_model || "gemini-2.5-flash");
    }
    if (!state.selectedRepo && payload.selected_github_repo) {
      setState("selectedRepo", payload.selected_github_repo);
    }
  }

  async function refreshAgents(): Promise<void> {
    const payload = await apiGet<SubAgent[]>("/api/agents");
    setState("agents", payload);
    if (state.selectedAgentId && !payload.find((entry) => entry.id === state.selectedAgentId)) {
      setState("selectedAgentId", "");
    }
  }

  async function refreshGates(): Promise<void> {
    const payload = await apiGet<GateSnapshot>("/api/gates");
    setState("gates", payload);
    setState("gateDraft", { ...payload.conditions });
  }

  async function refreshEvents(): Promise<void> {
    const payload = await apiGet<CrystalBallEvent[]>(`/api/events?limit=${state.eventsLimit}`);
    setState("events", payload);
  }

  async function refreshChatHistory(): Promise<void> {
    const key = state.selectedAgentId;
    const query = key ? `?agent_id=${encodeURIComponent(key)}&limit=100` : "?limit=100";
    const payload = await apiGet<ChatHistoryResponse>(`/api/chat/history${query}`);
    setState("chatHistory", payload.messages ?? []);
  }

  async function refreshCrystalHealth(): Promise<void> {
    const payload = await apiGet<CrystalBallHealth>("/api/crystal-ball/health");
    setState("crystalHealth", payload);
  }

  async function validateCrystal(): Promise<void> {
    const payload = await apiGet<CrystalBallValidation>("/api/crystal-ball/validate");
    setState("crystalValidation", payload);
  }

  async function smokeCrystal(): Promise<void> {
    const payload = await apiPost<CrystalBallSmoke>("/api/crystal-ball/smoke");
    setState("crystalSmoke", payload);
  }

  async function auditCrystal(): Promise<void> {
    const payload = await apiGet<CrystalBallAudit>("/api/crystal-ball/audit");
    setState("crystalAudit", payload);
  }

  async function refreshGithub(): Promise<void> {
    const status = await apiGet<GitHubStatus>("/api/github/status");
    setState("githubStatus", status);
    const reposPayload = await apiGet<GitHubReposResponse>("/api/github/repos?limit=120");
    setState("githubRepos", reposPayload.repos ?? []);
  }

  async function refreshVault(): Promise<void> {
    const payload = await apiGet<VaultStatus>("/api/vault/status");
    setState("vaultStatus", payload);
  }

  async function refreshSecrets(): Promise<void> {
    const payload = await apiGet<SecretMetadata[]>("/api/secrets");
    setState("secrets", payload);
  }

  async function refreshOauthStatuses(): Promise<void> {
    const updates: Record<string, OAuthStatus | null> = {};
    for (const provider of OAUTH_PROVIDERS) {
      try {
        updates[provider] = await apiGet<OAuthStatus>(`/api/oauth/${provider}/status`);
      } catch {
        updates[provider] = null;
      }
    }
    setState("oauth", updates);
  }

  async function refreshAll(): Promise<void> {
    await Promise.all([
      refreshHealth(),
      refreshSettings(),
      refreshAgents(),
      refreshGates(),
      refreshEvents(),
      refreshChatHistory(),
      refreshCrystalHealth(),
      refreshGithub(),
      refreshVault(),
      refreshSecrets(),
      refreshOauthStatuses()
    ]);
  }

  function persistWorkspaceTiles(next: WorkspaceTile[]): void {
    localStorage.setItem("kaizen.workspace.tiles", JSON.stringify(next));
  }

  function addWorkspaceTile(): void {
    const path = state.workspaceInput.trim();
    if (!path) {
      return;
    }
    if (state.workspaceTiles.some((tile) => tile.path.toLowerCase() === path.toLowerCase())) {
      pushNotice("warning", "Workspace path already attached.");
      return;
    }
    const next = [...state.workspaceTiles, { id: uid("ws"), path }];
    setState("workspaceTiles", next);
    setState("workspaceInput", "");
    persistWorkspaceTiles(next);
  }

  function removeWorkspaceTile(id: string): void {
    const next = state.workspaceTiles.filter((tile) => tile.id !== id);
    setState("workspaceTiles", next);
    persistWorkspaceTiles(next);
  }

  function parseWrapTargets(raw: string): Array<{ provider: string; model: string }> {
    return raw
      .split(",")
      .map((item) => item.trim())
      .filter((item) => item.length > 0)
      .map((item) => {
        const [provider, ...modelParts] = item.split(":");
        return { provider: provider.trim(), model: modelParts.join(":").trim() };
      })
      .filter((item) => item.provider.length > 0 && item.model.length > 0);
  }

  async function sendChat(): Promise<void> {
    const message = state.chatMessage.trim();
    if (!message) {
      return;
    }

    await runTask("sendChat", async () => {
      const payload: Record<string, unknown> = {
        message,
        clear_history: state.clearHistory,
        mode: state.chatMode,
        provider: state.chatProvider,
        model: state.chatModel
      };

      if (state.selectedAgentId) {
        payload.agent_id = state.selectedAgentId;
      }

      if (state.wrapMode) {
        const targets = parseWrapTargets(state.wrapTargets);
        if (targets.length === 0) {
          throw new Error("Wrap mode requires at least one target in provider:model format.");
        }
        payload.wrap_mode = true;
        payload.selected_models = targets;
      }

      const response = await apiPost<ChatResponse>("/api/chat", payload);

      setState("chatHistory", (messages) => [...messages, { role: "user", content: message }]);
      setState("chatHistory", (messages) => [...messages, { role: "assistant", content: response.reply }]);
      setState("chatMessage", "");
      setState("lastChatMeta", `${response.provider ?? "unknown"} / ${response.model ?? "unknown"}`);

      await Promise.all([refreshAgents(), refreshGates(), refreshEvents()]);
      pushNotice("success", "Message sent.");
    });
  }

  async function createAgent(): Promise<void> {
    const name = state.newAgentName.trim();
    const taskId = state.newAgentTaskId.trim();
    const objective = state.newAgentObjective.trim();

    if (!name || !taskId || !objective) {
      pushNotice("warning", "Provide name, task ID, and objective to create an agent.");
      return;
    }

    await runTask("createAgent", async () => {
      await apiPost<SubAgent>("/api/agents", {
        agent_name: name,
        task_id: taskId,
        objective,
        user_requested: true
      });
      setState("newAgentName", "");
      setState("newAgentTaskId", "");
      setState("newAgentObjective", "");
      await refreshAgents();
      pushNotice("success", "Agent created.");
    });
  }

  async function renameAgent(agent: SubAgent): Promise<void> {
    const next = window.prompt("New agent name", agent.name);
    if (!next || next.trim().length === 0 || next.trim() === agent.name) {
      return;
    }

    await runTask(`rename-${agent.id}`, async () => {
      await apiPatch<SubAgent>(`/api/agents/${encodeURIComponent(agent.id)}`, { name: next.trim() });
      await refreshAgents();
      pushNotice("success", `Renamed ${agent.name}.`);
    });
  }

  async function patchAgentStatus(agentId: string, status: AgentStatus): Promise<void> {
    await runTask(`status-${agentId}`, async () => {
      await apiPatch<SubAgent>(`/api/agents/${encodeURIComponent(agentId)}/status`, {
        status,
        kaizen_review_approved: state.gateDraft?.kaizen_review_approved ?? false
      });
      await refreshAgents();
      await refreshEvents();
      pushNotice("success", `Agent status updated to ${status}.`);
    });
  }

  async function stopAgent(agentId: string): Promise<void> {
    await runTask(`stop-${agentId}`, async () => {
      await apiPost<SubAgent>(`/api/agents/${encodeURIComponent(agentId)}/stop`);
      await refreshAgents();
      await refreshEvents();
      pushNotice("warning", `Agent ${agentId} stopped.`);
    });
  }

  async function clearAgent(agentId: string): Promise<void> {
    await runTask(`clear-${agentId}`, async () => {
      await apiPost<unknown>(`/api/agents/${encodeURIComponent(agentId)}/clear`);
      if (state.selectedAgentId === agentId) {
        await refreshChatHistory();
      }
      pushNotice("success", `Cleared conversation for ${agentId}.`);
    });
  }

  async function removeAgent(agentId: string): Promise<void> {
    const ok = window.confirm(`Remove agent ${agentId}? This cannot be undone.`);
    if (!ok) {
      return;
    }
    await runTask(`remove-${agentId}`, async () => {
      await apiDelete(`/api/agents/${encodeURIComponent(agentId)}`);
      await refreshAgents();
      await refreshEvents();
      if (state.selectedAgentId === agentId) {
        setState("selectedAgentId", "");
        await refreshChatHistory();
      }
      pushNotice("warning", `Removed agent ${agentId}.`);
    });
  }

  function updateGateDraft(key: keyof GateConditions, value: boolean): void {
    if (!state.gateDraft) {
      return;
    }
    setState("gateDraft", { ...state.gateDraft, [key]: value });
  }

  async function saveGateConditions(): Promise<void> {
    if (!state.gateDraft) {
      return;
    }
    await runTask("saveGates", async () => {
      const payload = await apiPatch<GateSnapshot>("/api/gates/conditions", state.gateDraft);
      setState("gates", payload);
      setState("gateDraft", { ...payload.conditions });
      await refreshEvents();
      pushNotice("success", "Gate conditions saved.");
    });
  }

  async function advanceGates(): Promise<void> {
    await runTask("advanceGates", async () => {
      const payload = await apiPost<TransitionResult>("/api/gates/advance");
      setState("gateTransition", payload);
      await Promise.all([refreshGates(), refreshEvents()]);
      if (payload.allowed) {
        pushNotice("success", `Gate advanced to ${payload.to}.`);
      } else {
        pushNotice("warning", `Gate blocked: ${payload.blocked_by.join(", ") || "Unknown reason"}`);
      }
    });
  }

  async function saveRepoSelection(): Promise<void> {
    await runTask("saveRepo", async () => {
      await apiPatch<KaizenSettings>("/api/settings", { selected_github_repo: state.selectedRepo });
      await refreshSettings();
      pushNotice("success", "Selected repository saved.");
    });
  }

  async function saveSecret(): Promise<void> {
    const provider = state.secretProvider.trim();
    const value = state.secretValue.trim();
    if (!provider || !value) {
      pushNotice("warning", "Provider and secret value are required.");
      return;
    }

    await runTask("saveSecret", async () => {
      await apiPut<SecretMetadata>(`/api/secrets/${encodeURIComponent(provider)}`, {
        value,
        secret_type: state.secretType
      });
      setState("secretValue", "");
      await refreshSecrets();
      pushNotice("success", `${provider} secret stored.`);
    });
  }

  async function testSecret(provider: string): Promise<void> {
    await runTask(`test-secret-${provider}`, async () => {
      const result = await apiPost<{ test_passed: boolean; error: string | null }>(
        `/api/secrets/${encodeURIComponent(provider)}/test`
      );
      if (result.test_passed) {
        pushNotice("success", `${provider} secret test passed.`);
      } else {
        pushNotice("warning", result.error || `${provider} secret test failed.`);
      }
    });
  }

  async function revealSecret(provider: string): Promise<void> {
    const ok = window.confirm(
      `Reveal decrypted secret for ${provider}? This displays it locally in this session.`
    );
    if (!ok) {
      return;
    }

    await runTask(`reveal-secret-${provider}`, async () => {
      const result = await apiGet<{ provider: string; key: string }>(
        `/api/secrets/${encodeURIComponent(provider)}/use`
      );
      setState("revealedSecret", `${result.provider}: ${result.key}`);
      pushNotice("warning", `Secret revealed for ${provider}.`);
    });
  }

  async function revokeSecret(provider: string): Promise<void> {
    const ok = window.confirm(`Revoke secret for ${provider}?`);
    if (!ok) {
      return;
    }

    await runTask(`revoke-secret-${provider}`, async () => {
      await apiDelete(`/api/secrets/${encodeURIComponent(provider)}`);
      await refreshSecrets();
      pushNotice("warning", `${provider} secret revoked.`);
    });
  }

  async function disconnectOauth(provider: string): Promise<void> {
    await runTask(`oauth-disconnect-${provider}`, async () => {
      await apiDelete(`/api/oauth/${encodeURIComponent(provider)}`);
      await refreshOauthStatuses();
      pushNotice("success", `${provider} OAuth disconnected.`);
    });
  }

  async function tryStartOauth(provider: string): Promise<void> {
    const key = `oauth-start-${provider}`;
    setBusy(key, true);
    try {
      const response = await apiGet<{ redirect_url?: string }>(
        `/api/oauth/${encodeURIComponent(provider)}/start`
      );
      if (response.redirect_url) {
        window.open(response.redirect_url, "_blank", "noopener,noreferrer");
      }
      pushNotice("info", `${provider} OAuth start requested.`);
    } catch (error) {
      const message = parseError(error);
      if (message.toLowerCase().includes("not implemented")) {
        pushNotice("info", `${provider} OAuth start is not implemented in backend yet.`);
      } else {
        pushNotice("error", message);
      }
    } finally {
      setBusy(key, false);
    }
  }

  async function tryRefreshOauth(provider: string): Promise<void> {
    const key = `oauth-refresh-${provider}`;
    setBusy(key, true);
    try {
      await apiPost<unknown>(`/api/oauth/${encodeURIComponent(provider)}/refresh`);
      await refreshOauthStatuses();
      pushNotice("info", `${provider} OAuth refresh requested.`);
    } catch (error) {
      const message = parseError(error);
      if (message.toLowerCase().includes("not implemented")) {
        pushNotice("info", `${provider} OAuth refresh is not implemented in backend yet.`);
      } else {
        pushNotice("error", message);
      }
    } finally {
      setBusy(key, false);
    }
  }

  function updateSettingsDraft<K extends keyof KaizenSettings>(key: K, value: KaizenSettings[K]): void {
    setState("settingsDraft", key, value);
  }

  async function saveSettings(): Promise<void> {
    await runTask("saveSettings", async () => {
      const payload = await apiPatch<KaizenSettings>("/api/settings", state.settingsDraft);
      setState("settings", payload);
      setState("settingsDraft", { ...payload });
      pushNotice("success", "Settings saved.");
    });
  }

  function resetSettingsDraft(): void {
    if (!state.settings) {
      return;
    }
    setState("settingsDraft", { ...state.settings });
    pushNotice("info", "Settings draft reset.");
  }

  const activeModes = createMemo(() => (state.selectedAgentId ? SUBAGENT_MODES : KAIZEN_MODES));

  const modelHints = createMemo(() => {
    if (!state.chatProvider) {
      return [];
    }
    return PROVIDER_MODEL_HINTS[state.chatProvider] || [];
  });

  const currentAgent = createMemo(() => state.agents.find((entry) => entry.id === state.selectedAgentId) || null);

  createEffect(() => {
    localStorage.setItem("kaizen.admin.token", adminToken());
  });

  createEffect(() => {
    const selected = state.selectedAgentId;
    void selected;
    runTask("history", refreshChatHistory);
  });

  createEffect(() => {
    if (!state.chatProvider && state.settings?.inference_provider) {
      setState("chatProvider", state.settings.inference_provider);
    }
    if (!state.chatModel && state.settings?.inference_model) {
      setState("chatModel", state.settings.inference_model);
    }
  });

  createEffect(() => {
    if (!activeModes().includes(state.chatMode)) {
      setState("chatMode", activeModes()[0] || "yolo");
    }
  });

  onMount(() => {
    void runTask("boot", refreshAll);

    const runtimeTicker = window.setInterval(() => {
      void runTask("runtime-poll", async () => {
        await Promise.all([refreshHealth(), refreshAgents(), refreshGates(), refreshEvents()]);
      });
    }, 5000);

    onCleanup(() => {
      window.clearInterval(runtimeTicker);
    });
  });

  return (
    <div class="app-shell">
      <aside class="nav-rail">
        <div class="brand">
          <div class="brand-dot" />
          <div>
            <div class="brand-title">Kaizen MAX</div>
            <div class="brand-subtitle">Mission Control</div>
          </div>
        </div>

        <nav class="tab-list">
          <For each={TABS}>
            {(tab) => (
              <button
                class={`tab-button ${activeTab() === tab.id ? "active" : ""}`}
                onClick={() => setActiveTab(tab.id)}
              >
                {tab.label}
              </button>
            )}
          </For>
        </nav>

        <div class="rail-actions">
          <button class="btn ghost" onClick={() => void runTask("refresh-all", refreshAll)} disabled={!!busy["refresh-all"]}>
            Refresh All
          </button>
        </div>
      </aside>

      <section class="main-shell">
        <header class="top-bar">
          <div class="status-strip">
            <span class={`status-chip ${state.health?.status === "ok" ? "ok" : "warn"}`}>
              {state.health ? `${state.health.engine} ${state.health.version}` : "Backend pending"}
            </span>
            <span class="status-chip neutral">Gate: {state.gates?.current_state || "unknown"}</span>
            <span class="status-chip neutral">Agents: {state.agents.length}</span>
            <span class="status-chip neutral">Events: {state.events.length}</span>
          </div>

          <div class="admin-token">
            <label for="admin-token-input">Admin token</label>
            <input
              id="admin-token-input"
              type="password"
              value={adminToken()}
              onInput={(event) => setAdminToken(event.currentTarget.value)}
              placeholder="Optional unless ADMIN_API_TOKEN is enabled"
            />
          </div>
        </header>

        <main class="tab-panel">
          <Show when={activeTab() === "mission"}>
            <section class="mission-layout">
              <article class="card chat-card">
                <div class="card-head">
                  <h2>{currentAgent() ? `Chat · ${currentAgent()!.name}` : "Chat · Kaizen"}</h2>
                  <div class="meta-row">{state.lastChatMeta || "No replies yet"}</div>
                </div>

                <div class="chat-controls">
                  <label>
                    Target
                    <select
                      value={state.selectedAgentId}
                      onChange={(event) => setState("selectedAgentId", event.currentTarget.value)}
                    >
                      <option value="">kaizen</option>
                      <For each={state.agents}>{(agent) => <option value={agent.id}>{agent.name}</option>}</For>
                    </select>
                  </label>

                  <label>
                    Mode
                    <select value={state.chatMode} onChange={(event) => setState("chatMode", event.currentTarget.value)}>
                      <For each={activeModes()}>{(mode) => <option value={mode}>{mode}</option>}</For>
                    </select>
                  </label>

                  <label>
                    Provider
                    <input
                      value={state.chatProvider}
                      list="provider-hints"
                      onInput={(event) => setState("chatProvider", event.currentTarget.value.trim())}
                    />
                  </label>

                  <label>
                    Model
                    <input
                      value={state.chatModel}
                      list="model-hints"
                      onInput={(event) => setState("chatModel", event.currentTarget.value.trim())}
                    />
                  </label>

                  <label class="check">
                    <input
                      type="checkbox"
                      checked={state.wrapMode}
                      onChange={(event) => setState("wrapMode", event.currentTarget.checked)}
                    />
                    Wrap mode
                  </label>

                  <label class="check">
                    <input
                      type="checkbox"
                      checked={state.clearHistory}
                      onChange={(event) => setState("clearHistory", event.currentTarget.checked)}
                    />
                    Clear history
                  </label>

                  <button class="btn ghost" onClick={() => void runTask("history-manual", refreshChatHistory)}>
                    Refresh History
                  </button>
                </div>

                <Show when={state.wrapMode}>
                  <div class="wrap-targets">
                    <label>
                      Wrap targets
                      <input
                        value={state.wrapTargets}
                        onInput={(event) => setState("wrapTargets", event.currentTarget.value)}
                        placeholder="anthropic:claude-sonnet-4-20250514, gemini:gemini-2.5-flash"
                      />
                    </label>
                  </div>
                </Show>

                <datalist id="provider-hints">
                  <For each={Object.keys(PROVIDER_MODEL_HINTS)}>{(provider) => <option value={provider} />}</For>
                </datalist>
                <datalist id="model-hints">
                  <For each={modelHints()}>{(model) => <option value={model} />}</For>
                </datalist>

                <div class="chat-log">
                  <Show
                    when={state.chatHistory.length > 0}
                    fallback={<div class="chat-empty">No messages yet. Send the first mission prompt.</div>}
                  >
                    <For each={state.chatHistory}>
                      {(message) => (
                        <div class={`chat-row ${message.role === "assistant" ? "assistant" : "user"}`}>
                          <div class="chat-role">{message.role}</div>
                          <pre>{message.content}</pre>
                        </div>
                      )}
                    </For>
                  </Show>
                </div>

                <div class="composer">
                  <textarea
                    value={state.chatMessage}
                    onInput={(event) => setState("chatMessage", event.currentTarget.value)}
                    placeholder="Give Kaizen the next objective..."
                  />
                  <button class="btn primary" onClick={() => void sendChat()} disabled={!!busy.sendChat}>
                    {busy.sendChat ? "Sending..." : "Send"}
                  </button>
                </div>
              </article>

              <article class="card agent-card">
                <div class="card-head">
                  <h2>Agents</h2>
                  <span>{state.agents.length} active</span>
                </div>

                <div class="new-agent-grid">
                  <input
                    value={state.newAgentName}
                    onInput={(event) => setState("newAgentName", event.currentTarget.value)}
                    placeholder="Agent name"
                  />
                  <input
                    value={state.newAgentTaskId}
                    onInput={(event) => setState("newAgentTaskId", event.currentTarget.value)}
                    placeholder="Task ID"
                  />
                  <input
                    value={state.newAgentObjective}
                    onInput={(event) => setState("newAgentObjective", event.currentTarget.value)}
                    placeholder="Objective"
                  />
                  <button class="btn primary" onClick={() => void createAgent()} disabled={!!busy.createAgent}>
                    Add Agent
                  </button>
                </div>

                <div class="agent-list">
                  <For each={state.agents}>
                    {(agent) => (
                      <div class={`agent-row ${state.selectedAgentId === agent.id ? "selected" : ""}`}>
                        <div class="agent-main">
                          <button class="link" onClick={() => setState("selectedAgentId", agent.id)}>
                            {agent.name}
                          </button>
                          <div class="agent-meta">{agent.task_id}</div>
                          <div class="agent-meta muted">{agent.objective}</div>
                        </div>

                        <div class="agent-controls">
                          <select
                            value={agent.status}
                            onChange={(event) =>
                              void patchAgentStatus(agent.id, event.currentTarget.value as AgentStatus)
                            }
                          >
                            <For each={AGENT_STATUSES}>
                              {(status) => <option value={status}>{status}</option>}
                            </For>
                          </select>

                          <button class="btn ghost" onClick={() => void renameAgent(agent)}>
                            Rename
                          </button>
                          <button class="btn ghost" onClick={() => void stopAgent(agent.id)}>
                            Stop
                          </button>
                          <button class="btn ghost" onClick={() => void clearAgent(agent.id)}>
                            Clear
                          </button>
                          <button class="btn danger" onClick={() => void removeAgent(agent.id)}>
                            Remove
                          </button>
                        </div>
                      </div>
                    )}
                  </For>
                </div>
              </article>
            </section>
          </Show>

          <Show when={activeTab() === "gates"}>
            <section class="card single">
              <div class="card-head">
                <h2>Workflow Gates</h2>
                <span>State: {state.gates?.current_state || "unknown"}</span>
              </div>

              <div class="gate-grid">
                <For each={Object.keys(GATE_LABELS) as Array<keyof GateConditions>}>
                  {(key) => (
                    <label class="check gate-check">
                      <input
                        type="checkbox"
                        checked={Boolean(state.gateDraft?.[key])}
                        onChange={(event) => updateGateDraft(key, event.currentTarget.checked)}
                      />
                      {GATE_LABELS[key]}
                    </label>
                  )}
                </For>
              </div>

              <div class="inline-actions">
                <button class="btn primary" onClick={() => void saveGateConditions()} disabled={!!busy.saveGates}>
                  Save Conditions
                </button>
                <button class="btn ghost" onClick={() => void advanceGates()} disabled={!!busy.advanceGates}>
                  Advance Workflow
                </button>
              </div>

              <Show when={state.gateTransition}>
                <div class="result-block">
                  <div>
                    Transition: {state.gateTransition?.from} → {state.gateTransition?.to}
                  </div>
                  <Show when={state.gateTransition?.allowed} fallback={<div>Blocked by: {state.gateTransition?.blocked_by.join(", ") || "unknown"}</div>}>
                    <div>Transition allowed.</div>
                  </Show>
                </div>
              </Show>
            </section>
          </Show>

          <Show when={activeTab() === "activity"}>
            <section class="card single">
              <div class="card-head">
                <h2>Crystal Ball Activity</h2>
                <div class="inline-actions">
                  <label>
                    Limit
                    <select
                      value={state.eventsLimit}
                      onChange={(event) => setState("eventsLimit", Number(event.currentTarget.value) || 100)}
                    >
                      <option value={50}>50</option>
                      <option value={100}>100</option>
                      <option value={250}>250</option>
                      <option value={500}>500</option>
                    </select>
                  </label>
                  <button class="btn ghost" onClick={() => void runTask("events-refresh", refreshEvents)}>
                    Refresh Events
                  </button>
                </div>
              </div>

              <div class="inline-actions wrap">
                <button class="btn ghost" onClick={() => void runTask("cb-health", refreshCrystalHealth)}>
                  Health
                </button>
                <button class="btn ghost" onClick={() => void runTask("cb-validate", validateCrystal)}>
                  Validate
                </button>
                <button class="btn ghost" onClick={() => void runTask("cb-smoke", smokeCrystal)}>
                  Smoke
                </button>
                <button class="btn ghost" onClick={() => void runTask("cb-audit", auditCrystal)}>
                  Audit
                </button>
              </div>

              <div class="grid-two">
                <div class="result-block compact">
                  <h3>Health</h3>
                  <pre>{JSON.stringify(state.crystalHealth, null, 2)}</pre>
                </div>
                <div class="result-block compact">
                  <h3>Validation / Smoke / Audit</h3>
                  <pre>{JSON.stringify({ validate: state.crystalValidation, smoke: state.crystalSmoke, audit: state.crystalAudit }, null, 2)}</pre>
                </div>
              </div>

              <div class="event-feed">
                <For each={state.events}>
                  {(eventItem) => (
                    <div class="event-row">
                      <div class="event-head">
                        <span>{eventItem.event_type}</span>
                        <span>{eventItem.timestamp}</span>
                      </div>
                      <div class="event-body">{eventItem.message}</div>
                      <div class="event-meta">
                        {eventItem.source_actor} → {eventItem.target_actor} · {eventItem.task_id}
                      </div>
                    </div>
                  )}
                </For>
              </div>
            </section>
          </Show>

          <Show when={activeTab() === "workspace"}>
            <section class="card single">
              <div class="card-head">
                <h2>Workspace & GitHub</h2>
                <button class="btn ghost" onClick={() => void runTask("github-refresh", refreshGithub)}>
                  Refresh
                </button>
              </div>

              <div class="result-block compact">
                <h3>GitHub status</h3>
                <pre>{JSON.stringify(state.githubStatus, null, 2)}</pre>
              </div>

              <div class="inline-actions wrap">
                <label>
                  Selected repo
                  <select
                    value={state.selectedRepo}
                    onChange={(event) => setState("selectedRepo", event.currentTarget.value)}
                  >
                    <option value="">(none)</option>
                    <For each={state.githubRepos}>
                      {(repo) => <option value={repo.name_with_owner}>{repo.name_with_owner}</option>}
                    </For>
                  </select>
                </label>
                <button class="btn primary" onClick={() => void saveRepoSelection()}>
                  Save Repo in Settings
                </button>
              </div>

              <div class="workspace-tiles">
                <h3>Local workspace tiles</h3>
                <div class="inline-actions">
                  <input
                    value={state.workspaceInput}
                    onInput={(event) => setState("workspaceInput", event.currentTarget.value)}
                    placeholder="C:\\Projects\\my-repo"
                  />
                  <button class="btn ghost" onClick={addWorkspaceTile}>
                    Attach
                  </button>
                </div>

                <For each={state.workspaceTiles}>
                  {(tile) => (
                    <div class="tile-row">
                      <span>{tile.path}</span>
                      <button class="btn danger" onClick={() => removeWorkspaceTile(tile.id)}>
                        Remove
                      </button>
                    </div>
                  )}
                </For>
              </div>
            </section>
          </Show>

          <Show when={activeTab() === "integrations"}>
            <section class="card single">
              <div class="card-head">
                <h2>Providers & Secrets</h2>
                <div class="inline-actions">
                  <button class="btn ghost" onClick={() => void runTask("vault-refresh", refreshVault)}>
                    Vault
                  </button>
                  <button class="btn ghost" onClick={() => void runTask("secrets-refresh", refreshSecrets)}>
                    Secrets
                  </button>
                  <button class="btn ghost" onClick={() => void runTask("oauth-refresh", refreshOauthStatuses)}>
                    OAuth
                  </button>
                </div>
              </div>

              <div class="result-block compact">
                <h3>Vault status</h3>
                <pre>{JSON.stringify(state.vaultStatus, null, 2)}</pre>
              </div>

              <div class="grid-two">
                <div>
                  <h3>Store secret</h3>
                  <div class="form-grid">
                    <label>
                      Provider
                      <select
                        value={state.secretProvider}
                        onChange={(event) => setState("secretProvider", event.currentTarget.value)}
                      >
                        <For each={SECRET_PROVIDERS}>{(provider) => <option value={provider}>{provider}</option>}</For>
                      </select>
                    </label>
                    <label>
                      Secret type
                      <input
                        value={state.secretType}
                        onInput={(event) => setState("secretType", event.currentTarget.value)}
                      />
                    </label>
                    <label class="full">
                      Secret value
                      <input
                        type="password"
                        value={state.secretValue}
                        onInput={(event) => setState("secretValue", event.currentTarget.value)}
                        placeholder="Paste provider key"
                      />
                    </label>
                    <button class="btn primary" onClick={() => void saveSecret()} disabled={!!busy.saveSecret}>
                      Save Secret
                    </button>
                  </div>
                </div>

                <div>
                  <h3>OAuth providers</h3>
                  <For each={OAUTH_PROVIDERS}>
                    {(provider) => (
                      <div class="oauth-row">
                        <div>
                          <strong>{provider}</strong>
                          <div class="agent-meta muted">
                            {state.oauth[provider]
                              ? state.oauth[provider]!.message
                              : "No status (or endpoint unavailable)"}
                          </div>
                        </div>
                        <div class="inline-actions">
                          <button
                            class="btn ghost"
                            onClick={() => void tryStartOauth(provider)}
                            disabled={!!busy[`oauth-start-${provider}`]}
                          >
                            Start
                          </button>
                          <button
                            class="btn ghost"
                            onClick={() => void tryRefreshOauth(provider)}
                            disabled={!!busy[`oauth-refresh-${provider}`]}
                          >
                            Refresh
                          </button>
                          <button class="btn danger" onClick={() => void disconnectOauth(provider)}>
                            Disconnect
                          </button>
                        </div>
                      </div>
                    )}
                  </For>
                </div>
              </div>

              <h3>Secrets</h3>
              <div class="secret-list">
                <For each={state.secrets}>
                  {(secret) => (
                    <div class="secret-row">
                      <div>
                        <strong>{secret.provider}</strong>
                        <div class="agent-meta muted">
                          key {secret.last4} · {secret.secret_type} · updated {secret.last_updated}
                        </div>
                      </div>
                      <div class="inline-actions">
                        <button class="btn ghost" onClick={() => void testSecret(secret.provider)}>
                          Test
                        </button>
                        <button class="btn ghost" onClick={() => void revealSecret(secret.provider)}>
                          Use
                        </button>
                        <button class="btn danger" onClick={() => void revokeSecret(secret.provider)}>
                          Revoke
                        </button>
                      </div>
                    </div>
                  )}
                </For>
              </div>

              <Show when={state.revealedSecret}>
                <div class="result-block">
                  <h3>Revealed secret (current session)</h3>
                  <pre>{state.revealedSecret}</pre>
                </div>
              </Show>
            </section>
          </Show>

          <Show when={activeTab() === "settings"}>
            <section class="card single">
              <div class="card-head">
                <h2>System Settings</h2>
                <div class="inline-actions">
                  <button class="btn ghost" onClick={resetSettingsDraft}>
                    Reset Draft
                  </button>
                  <button class="btn primary" onClick={() => void saveSettings()} disabled={!!busy.saveSettings}>
                    Save Settings
                  </button>
                </div>
              </div>

              <div class="settings-grid">
                <label>
                  Runtime engine
                  <input
                    value={String(state.settingsDraft.runtime_engine ?? "")}
                    onInput={(event) => updateSettingsDraft("runtime_engine", event.currentTarget.value)}
                  />
                </label>

                <label>
                  Inference provider
                  <input
                    value={String(state.settingsDraft.inference_provider ?? "")}
                    onInput={(event) => updateSettingsDraft("inference_provider", event.currentTarget.value)}
                  />
                </label>

                <label>
                  Inference model
                  <input
                    value={String(state.settingsDraft.inference_model ?? "")}
                    onInput={(event) => updateSettingsDraft("inference_model", event.currentTarget.value)}
                  />
                </label>

                <label>
                  Max subagents
                  <input
                    type="number"
                    value={Number(state.settingsDraft.max_subagents ?? 1)}
                    onInput={(event) => updateSettingsDraft("max_subagents", Number(event.currentTarget.value) || 1)}
                  />
                </label>

                <label>
                  Max tokens
                  <input
                    type="number"
                    value={Number(state.settingsDraft.inference_max_tokens ?? 4096)}
                    onInput={(event) =>
                      updateSettingsDraft("inference_max_tokens", Number(event.currentTarget.value) || 4096)
                    }
                  />
                </label>

                <label>
                  Temperature
                  <input
                    type="number"
                    step="0.05"
                    min="0"
                    max="1"
                    value={Number(state.settingsDraft.inference_temperature ?? 0.7)}
                    onInput={(event) =>
                      updateSettingsDraft("inference_temperature", Number(event.currentTarget.value) || 0.7)
                    }
                  />
                </label>

                <label>
                  Mattermost URL
                  <input
                    value={String(state.settingsDraft.mattermost_url ?? "")}
                    onInput={(event) => updateSettingsDraft("mattermost_url", event.currentTarget.value)}
                  />
                </label>

                <label>
                  Mattermost channel ID
                  <input
                    value={String(state.settingsDraft.mattermost_channel_id ?? "")}
                    onInput={(event) => updateSettingsDraft("mattermost_channel_id", event.currentTarget.value)}
                  />
                </label>

                <label class="check">
                  <input
                    type="checkbox"
                    checked={Boolean(state.settingsDraft.hard_gates_enabled)}
                    onChange={(event) =>
                      updateSettingsDraft("hard_gates_enabled", event.currentTarget.checked)
                    }
                  />
                  Hard gates enabled
                </label>

                <label class="check">
                  <input
                    type="checkbox"
                    checked={Boolean(state.settingsDraft.require_human_smoke_test_before_deploy)}
                    onChange={(event) =>
                      updateSettingsDraft("require_human_smoke_test_before_deploy", event.currentTarget.checked)
                    }
                  />
                  Require human smoke test before deploy
                </label>

                <label class="check">
                  <input
                    type="checkbox"
                    checked={Boolean(state.settingsDraft.crystal_ball_enabled)}
                    onChange={(event) => updateSettingsDraft("crystal_ball_enabled", event.currentTarget.checked)}
                  />
                  Crystal Ball enabled
                </label>

                <label class="check">
                  <input
                    type="checkbox"
                    checked={Boolean(state.settingsDraft.auto_spawn_subagents)}
                    onChange={(event) => updateSettingsDraft("auto_spawn_subagents", event.currentTarget.checked)}
                  />
                  Auto spawn subagents
                </label>

                <label class="check">
                  <input
                    type="checkbox"
                    checked={Boolean(state.settingsDraft.provider_inference_only)}
                    onChange={(event) => updateSettingsDraft("provider_inference_only", event.currentTarget.checked)}
                  />
                  Provider inference only
                </label>

                <label class="check">
                  <input
                    type="checkbox"
                    checked={Boolean(state.settingsDraft.credentials_ui_enabled)}
                    onChange={(event) => updateSettingsDraft("credentials_ui_enabled", event.currentTarget.checked)}
                  />
                  Credentials UI enabled
                </label>
              </div>

              <div class="result-block compact">
                <h3>Current settings snapshot</h3>
                <pre>{JSON.stringify(state.settings, null, 2)}</pre>
              </div>
            </section>
          </Show>
        </main>
      </section>

      <div class="toast-stack">
        <For each={notices()}>
          {(notice) => <div class={`toast ${notice.kind}`}>{notice.text}</div>}
        </For>
      </div>
    </div>
  );
}
