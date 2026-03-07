import {
  For,
  Show,
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
  onMount
} from "solid-js";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { createStore } from "solid-js/store";
import { applyRepoUpdate, coreRequest, getRepoUpdateStatus, openExternalUrl } from "./lib/tauri";
import headerLogo from "./assets/branding/headerlogo.png";
import brandEmblem from "./assets/branding/logo-emblem.png";
import kaizenText from "./assets/branding/kaizen-text.png";
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
  RepoUpdateStatus,
  SubAgent,
  TransitionResult,
  ZeroclawControlPlane,
  ZeroclawProviderAction
} from "./lib/types";

type TabId = "mission" | "gates" | "activity" | "workspace" | "integrations" | "settings";

interface WorkspaceTile {
  id: string;
  path: string;
}

const TABS: Array<{ id: TabId; label: string }> = [
  { id: "mission", label: "Mission" },
  { id: "gates", label: "Workflow" },
  { id: "activity", label: "Status" },
  { id: "workspace", label: "Workspace" },
  { id: "integrations", label: "Zeroclaw" },
  { id: "settings", label: "Settings" }
];

const KAIZEN_MODES = ["yolo", "build", "plan", "reason", "orchestrator"];
const SUBAGENT_MODES = ["build", "plan"];
const AGENT_STATUSES: AgentStatus[] = ["idle", "active", "blocked", "review_pending", "done"];

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

function shortCommit(commit: string | null | undefined): string {
  if (!commit) {
    return "unknown";
  }
  return commit.slice(0, 7);
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
    updateStatus: null as RepoUpdateStatus | null,
    zeroclaw: null as ZeroclawControlPlane | null,
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
    selectedAgentId: "",
    chatHistory: [] as ChatMessage[],
    chatMessage: "",
    chatMode: "yolo",
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

  async function refreshZeroclaw(): Promise<void> {
    const payload = await apiGet<ZeroclawControlPlane>("/api/zeroclaw");
    setState("zeroclaw", payload);
  }

  async function refreshUpdateStatus(): Promise<void> {
    const payload = await getRepoUpdateStatus();
    setState("updateStatus", payload);
  }

  async function refreshAll(): Promise<void> {
    await Promise.allSettled([
      refreshHealth(),
      refreshUpdateStatus(),
      refreshZeroclaw(),
      refreshSettings(),
      refreshAgents(),
      refreshGates(),
      refreshEvents(),
      refreshChatHistory(),
      refreshCrystalHealth(),
      refreshGithub()
    ]);
  }

  async function applyReleaseUpdate(): Promise<void> {
    await runTask("apply-update", async () => {
      const status = await getRepoUpdateStatus();
      setState("updateStatus", status);
      if (!status.can_apply_update) {
        throw new Error(status.message);
      }

      await applyRepoUpdate();
      pushNotice("info", "Applying release update from origin/main. The app will relaunch after rebuild.");
      window.setTimeout(() => {
        void getCurrentWindow().close();
      }, 800);
    });
  }

  function scheduleZeroclawRefresh(attempts = 12, delayMs = 2500): void {
    const tick = (): void => {
      if (attempts <= 0) {
        return;
      }
      attempts -= 1;
      void Promise.allSettled([refreshZeroclaw(), refreshSettings()]).then(() => {
        if (attempts > 0 && !state.zeroclaw?.ready) {
          window.setTimeout(tick, delayMs);
        }
      });
    };

    window.setTimeout(tick, delayMs);
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
        provider: "zeroclaw",
        model: state.zeroclaw?.selected_model || state.settings?.inference_model || ""
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

  async function saveZeroclawProvider(provider: string): Promise<void> {
    await runTask(`zeroclaw-provider-${provider}`, async () => {
      const payload = await apiPatch<ZeroclawControlPlane>("/api/zeroclaw", { provider });
      setState("zeroclaw", payload);
      await refreshSettings();
      pushNotice("success", `${payload.providers.find((entry) => entry.id === provider)?.label || provider} selected.`);
    });
  }

  async function saveZeroclawModel(model: string): Promise<void> {
    await runTask(`zeroclaw-model-${model}`, async () => {
      const payload = await apiPatch<ZeroclawControlPlane>("/api/zeroclaw", { model });
      setState("zeroclaw", payload);
      await refreshSettings();
      pushNotice("success", "Model updated.");
    });
  }

  async function runProviderAction(provider: string, action: ZeroclawProviderAction): Promise<void> {
    await runTask(`provider-action-${provider}-${action.kind}`, async () => {
      if (action.kind === "connect") {
        const response = await apiPost<{ redirect_url?: string; message?: string }>(
          `/api/zeroclaw/providers/${encodeURIComponent(provider)}/connect`
        );
        if (response.redirect_url) {
          try {
            await openExternalUrl(response.redirect_url);
          } catch {
            window.open(response.redirect_url, "_blank", "noopener,noreferrer");
          }
        }
        scheduleZeroclawRefresh();
        pushNotice("info", response.message || "Continue in your browser.");
        return;
      }

      if (action.kind === "refresh") {
        const response = await apiPost<{ message?: string }>(
          `/api/zeroclaw/providers/${encodeURIComponent(provider)}/refresh`
        );
        await Promise.allSettled([refreshZeroclaw(), refreshSettings()]);
        pushNotice("success", response.message || "Connection refreshed.");
        return;
      }

      if (action.kind === "disconnect") {
        const response = await coreRequest<{ message?: string }>({
          method: "DELETE",
          path: `/api/zeroclaw/providers/${encodeURIComponent(provider)}`,
          adminToken: adminToken()
        });
        await Promise.allSettled([refreshZeroclaw(), refreshSettings()]);
        pushNotice("success", response.message || "Disconnected.");
      }
    });
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
    if (!activeModes().includes(state.chatMode)) {
      setState("chatMode", activeModes()[0] || "yolo");
    }
  });

  createEffect(() => {
    const updateStatus = state.updateStatus;
    if (!updateStatus?.update_available || !updateStatus.remote_commit) {
      return;
    }

    const noticeKey = "kaizen.update.last-notified";
    const lastNotified = localStorage.getItem(noticeKey);
    if (lastNotified === updateStatus.remote_commit) {
      return;
    }

    localStorage.setItem(noticeKey, updateStatus.remote_commit);
    pushNotice(
      "info",
      `Release update ready from main: ${shortCommit(updateStatus.remote_commit)} ${updateStatus.remote_subject || ""}`.trim()
    );
  });

  onMount(() => {
    void runTask("boot", refreshAll);

    const runtimeTicker = window.setInterval(() => {
      void runTask("runtime-poll", async () => {
        await Promise.all([refreshHealth(), refreshAgents(), refreshGates(), refreshEvents()]);
      });
    }, 5000);

    const updateTicker = window.setInterval(() => {
      void runTask("update-status", refreshUpdateStatus);
    }, 15 * 60 * 1000);

    onCleanup(() => {
      window.clearInterval(runtimeTicker);
      window.clearInterval(updateTicker);
    });
  });

  return (
    <div class="app-shell">
      <aside class="nav-rail">
        <div class="brand brand-panel">
          <div class="brand-mark">
            <img src={brandEmblem} alt="Kaizen emblem" />
          </div>
          <div class="brand-copy">
            <div class="brand-eyebrow">Kaizen Innovations</div>
            <div class="brand-title">Kaizen MAX</div>
            <div class="brand-subtitle">Mission Control</div>
          </div>
        </div>

        <div class="brand-banner">
          <img class="brand-banner-wordmark" src={kaizenText} alt="Kaizen Innovations" />
          <div class="brand-banner-copy">
            <div class="brand-banner-kicker">Native Command Surface</div>
            <p>
              Zeroclaw routing, Codex-first execution, and provider control without the old vault layer.
            </p>
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
          <div class="top-bar-main">
            <div class="top-bar-kicker">Kaizen Innovations / Zeroclaw Control Plane</div>
            <div class="top-bar-heading-row">
              <div>
                <h1 class="top-bar-heading">Mission Control</h1>
                <p class="top-bar-blurb">
                  {state.zeroclaw?.headline || "Choose a provider and model for Zeroclaw."}
                </p>
              </div>
              <img class="top-bar-logo" src={headerLogo} alt="" aria-hidden="true" />
            </div>

            <div class="status-strip">
              <span class={`status-chip ${state.health?.status === "ok" ? "ok" : "warn"}`}>
                {state.health ? `${state.health.engine} ${state.health.version}` : "Backend pending"}
              </span>
              <span
                class={`status-chip ${
                  state.updateStatus?.update_available
                    ? "warn"
                    : state.updateStatus?.enabled
                      ? "ok"
                      : "neutral"
                }`}
              >
                Release:{" "}
                {state.updateStatus?.update_available
                  ? `${state.updateStatus.behind_count} pending`
                  : state.updateStatus?.enabled
                    ? "current"
                    : "unavailable"}
              </span>
              <span class="status-chip neutral">Gate: {state.gates?.current_state || "unknown"}</span>
              <span class="status-chip neutral">Agents: {state.agents.length}</span>
              <span class="status-chip neutral">Events: {state.events.length}</span>
            </div>

            <Show when={state.updateStatus}>
              {(updateStatus) => (
                <section
                  class={`release-banner ${
                    updateStatus().update_available ? "available" : updateStatus().enabled ? "current" : "offline"
                  }`}
                >
                  <div class="release-banner-copy">
                    <div class="release-banner-kicker">Release Channel / {updateStatus().release_branch}</div>
                    <h2>
                      {updateStatus().update_available
                        ? "Update ready from main"
                        : updateStatus().enabled
                          ? "This install is on the current release"
                          : "Update checks are unavailable"}
                    </h2>
                    <p>{updateStatus().message}</p>
                    <div class="release-banner-meta">
                      <span>Local {shortCommit(updateStatus().local_commit)}</span>
                      <span>Remote {shortCommit(updateStatus().remote_commit)}</span>
                      <span>Branch {updateStatus().current_branch || "unknown"}</span>
                    </div>
                  </div>
                  <div class="release-banner-actions">
                    <button
                      class="btn ghost"
                      onClick={() => void runTask("update-status", refreshUpdateStatus)}
                      disabled={!!busy["update-status"]}
                    >
                      Check Updates
                    </button>
                    <button
                      class="btn"
                      onClick={() => void applyReleaseUpdate()}
                      disabled={!updateStatus().can_apply_update || !!busy["apply-update"]}
                    >
                      {busy["apply-update"] ? "Updating..." : "Apply Update"}
                    </button>
                  </div>
                </section>
              )}
            </Show>
          </div>

          <div class="top-bar-side">
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
                <h2>System Status</h2>
                <div class="inline-actions">
                  <button class="btn ghost" onClick={() => void runTask("status-refresh", refreshAll)}>
                    Refresh
                  </button>
                </div>
              </div>

              <div class="inline-actions wrap">
                <button class="btn ghost" onClick={() => void runTask("cb-validate", validateCrystal)}>
                  Check Bridge
                </button>
                <button class="btn ghost" onClick={() => void runTask("cb-smoke", smokeCrystal)}>
                  Send Test
                </button>
                <button class="btn ghost" onClick={() => void runTask("cb-audit", auditCrystal)}>
                  Verify Archive
                </button>
              </div>

              <div class="grid-two">
                <div class="summary-card">
                  <div class="summary-label">Backend</div>
                  <div class="summary-value">{state.health?.status === "ok" ? "Online" : "Checking"}</div>
                  <div class="summary-note">{state.health ? `${state.health.engine} ${state.health.version}` : "Starting"}</div>
                </div>
                <div class="summary-card">
                  <div class="summary-label">Zeroclaw</div>
                  <div class="summary-value">{state.zeroclaw?.ready ? "Ready" : "Needs setup"}</div>
                  <div class="summary-note">{state.zeroclaw?.detail || "Choose a provider to continue."}</div>
                </div>
                <div class="summary-card">
                  <div class="summary-label">Crystal Ball</div>
                  <div class="summary-value">
                    {state.crystalHealth?.enabled ? (state.crystalHealth.mattermost_connected ? "Connected" : "Local only") : "Off"}
                  </div>
                  <div class="summary-note">
                    {state.crystalHealth?.enabled
                      ? `${state.events.length} recent items available`
                      : "Bridge is turned off in settings."}
                  </div>
                </div>
                <div class="summary-card">
                  <div class="summary-label">Workspace</div>
                  <div class="summary-value">{state.workspaceTiles.length}</div>
                  <div class="summary-note">Attached local folders</div>
                </div>
              </div>              <div class="stack-list">
                <div class="soft-row">
                  <span>Mattermost bridge</span>
                  <strong>{state.crystalHealth?.mattermost_connected ? "Connected" : "Not connected"}</strong>
                </div>
                <div class="soft-row">
                  <span>Archive integrity</span>
                  <strong>{state.crystalAudit?.valid ?? state.crystalHealth?.archive_integrity_valid ? "Healthy" : "Needs review"}</strong>
                </div>
                <div class="soft-row">
                  <span>Recent activity</span>
                  <strong>{state.events.length} items</strong>
                </div>
                <Show when={state.crystalValidation?.error}>
                  <div class="soft-row">
                    <span>Bridge check</span>
                    <strong>{state.crystalValidation?.error}</strong>
                  </div>
                </Show>
                <Show when={state.crystalSmoke?.error}>
                  <div class="soft-row">
                    <span>Last test</span>
                    <strong>{state.crystalSmoke?.error}</strong>
                  </div>
                </Show>
                <Show when={state.crystalAudit?.reason}>
                  <div class="soft-row">
                    <span>Archive note</span>
                    <strong>{state.crystalAudit?.reason}</strong>
                  </div>
                </Show>
              </div>
            </section>
          </Show>

          <Show when={activeTab() === "workspace"}>
            <section class="card single">
              <div class="card-head">
                <h2>Workspace</h2>
                <button class="btn ghost" onClick={() => void runTask("github-refresh", refreshGithub)}>
                  Refresh
                </button>
              </div>

              <div class="grid-two">
                <div class="summary-card">
                  <div class="summary-label">GitHub</div>
                  <div class="summary-value">{state.githubStatus?.authenticated ? "Connected" : "Not connected"}</div>
                  <div class="summary-note">{state.githubStatus?.login || "Sign in with GitHub CLI on this device."}</div>
                </div>
                <div class="summary-card">
                  <div class="summary-label">Repositories</div>
                  <div class="summary-value">{state.githubRepos.length}</div>
                  <div class="summary-note">Available in your GitHub account</div>
                </div>
              </div>

              <div class="inline-actions wrap">
                <label>
                  Active repository
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
                  Save
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
          </Show>          <Show when={activeTab() === "integrations"}>
            <section class="card single">
              <div class="card-head">
                <h2>Zeroclaw</h2>
                <div class="inline-actions">
                  <button class="btn ghost" onClick={() => void runTask("zeroclaw-refresh", async () => {
                    await Promise.allSettled([refreshZeroclaw(), refreshSettings()]);
                  })}>
                    Refresh
                  </button>
                </div>
              </div>

              <div class="hero-panel">
                <div>
                  <div class="hero-kicker">Control Plane</div>
                  <h3>{state.zeroclaw?.headline || "Choose a provider"}</h3>
                  <p>{state.zeroclaw?.detail || "Zeroclaw keeps provider setup in one place."}</p>
                </div>
                <div class={`hero-badge ${state.zeroclaw?.ready ? "ready" : "pending"}`}>
                  {state.zeroclaw?.ready ? "Ready" : "Setup needed"}
                </div>
              </div>

              <div class="grid-two">
                <label>
                  Provider
                  <select
                    value={state.zeroclaw?.selected_provider || ""}
                    onChange={(event) => void saveZeroclawProvider(event.currentTarget.value)}
                  >
                    <For each={state.zeroclaw?.providers || []}>
                      {(provider) => <option value={provider.id}>{provider.label}</option>}
                    </For>
                  </select>
                </label>

                <label>
                  Model
                  <select
                    value={state.zeroclaw?.selected_model || ""}
                    onChange={(event) => void saveZeroclawModel(event.currentTarget.value)}
                  >
                    <For each={state.zeroclaw?.available_models || []}>
                      {(model) => <option value={model}>{model}</option>}
                    </For>
                  </select>
                </label>
              </div>

              <div class="provider-grid">
                <For each={state.zeroclaw?.providers || []}>
                  {(provider) => (
                    <article class={`provider-card ${provider.selected ? "selected" : ""}`}>
                      <div class="provider-card-head">
                        <div>
                          <h3>{provider.label}</h3>
                          <div class="provider-card-summary">{provider.summary}</div>
                        </div>
                        <span class={`provider-badge ${provider.ready ? "ready" : provider.connected ? "pending" : "idle"}`}>
                          {provider.badge}
                        </span>
                      </div>

                      <div class="provider-models">
                        <For each={provider.models}>
                          {(model) => <span class="provider-model-pill">{model}</span>}
                        </For>
                      </div>

                      <div class="inline-actions wrap">
                        <button class={`btn ${provider.selected ? "primary" : "ghost"}`} onClick={() => void saveZeroclawProvider(provider.id)}>
                          {provider.selected ? "Selected" : "Use This Provider"}
                        </button>
                        <Show when={provider.primary_action}>
                          {(action) => (
                            <button
                              class="btn ghost"
                              onClick={() => void runProviderAction(provider.id, action())}
                              disabled={!!busy[`provider-action-${provider.id}-${action().kind}`]}
                            >
                              {busy[`provider-action-${provider.id}-${action().kind}`] ? "Working..." : action().label}
                            </button>
                          )}
                        </Show>
                        <Show when={provider.secondary_action}>
                          {(action) => (
                            <button
                              class="btn danger"
                              onClick={() => void runProviderAction(provider.id, action())}
                              disabled={!!busy[`provider-action-${provider.id}-${action().kind}`]}
                            >
                              {busy[`provider-action-${provider.id}-${action().kind}`] ? "Working..." : action().label}
                            </button>
                          )}
                        </Show>
                      </div>
                    </article>
                  )}
                </For>
              </div>
            </section>
          </Show>          <Show when={activeTab() === "settings"}>
            <section class="card single">
              <div class="card-head">
                <h2>Settings</h2>
                <div class="inline-actions">
                  <button class="btn ghost" onClick={resetSettingsDraft}>
                    Reset
                  </button>
                  <button class="btn primary" onClick={() => void saveSettings()} disabled={!!busy.saveSettings}>
                    Save
                  </button>
                </div>
              </div>

              <div class="settings-grid">
                <label>
                  Max teammates
                  <input
                    type="number"
                    value={Number(state.settingsDraft.max_subagents ?? 1)}
                    onInput={(event) => updateSettingsDraft("max_subagents", Number(event.currentTarget.value) || 1)}
                  />
                </label>

                <label>
                  Reply size
                  <input
                    type="number"
                    value={Number(state.settingsDraft.inference_max_tokens ?? 4096)}
                    onInput={(event) => updateSettingsDraft("inference_max_tokens", Number(event.currentTarget.value) || 4096)}
                  />
                </label>

                <label>
                  Creativity
                  <input
                    type="number"
                    step="0.05"
                    min="0"
                    max="1"
                    value={Number(state.settingsDraft.inference_temperature ?? 0.7)}
                    onInput={(event) => updateSettingsDraft("inference_temperature", Number(event.currentTarget.value) || 0.7)}
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
                  Mattermost channel
                  <input
                    value={String(state.settingsDraft.mattermost_channel_id ?? "")}
                    onInput={(event) => updateSettingsDraft("mattermost_channel_id", event.currentTarget.value)}
                  />
                </label>

                <label class="check">
                  <input
                    type="checkbox"
                    checked={Boolean(state.settingsDraft.crystal_ball_enabled)}
                    onChange={(event) => updateSettingsDraft("crystal_ball_enabled", event.currentTarget.checked)}
                  />
                  Enable Crystal Ball bridge
                </label>

                <label class="check">
                  <input
                    type="checkbox"
                    checked={Boolean(state.settingsDraft.require_human_smoke_test_before_deploy)}
                    onChange={(event) => updateSettingsDraft("require_human_smoke_test_before_deploy", event.currentTarget.checked)}
                  />
                  Require a manual check before deploy
                </label>

                <label class="check">
                  <input
                    type="checkbox"
                    checked={Boolean(state.settingsDraft.auto_spawn_subagents)}
                    onChange={(event) => updateSettingsDraft("auto_spawn_subagents", event.currentTarget.checked)}
                  />
                  Let Kaizen start helpers automatically
                </label>
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



