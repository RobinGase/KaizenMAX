import { useEffect, useMemo, useState } from "react";
import {
  advanceGate,
  createAgent,
  fetchAgents,
  fetchCrystalBallAudit,
  fetchCrystalBallEvents,
  fetchCrystalBallHealth,
  fetchGateSnapshot,
  fetchSettings,
  patchGateConditions,
  patchSettings,
  renameAgent,
  runCrystalBallSmoke,
  sendChat,
  streamChat,
  updateAgentStatus,
  validateCrystalBall,
} from "./api";
import KaizenChat from "./components/KaizenChat";
import AgentChats from "./components/AgentChats";
import AgentPanel from "./components/AgentPanel";
import CrystalBall from "./components/CrystalBall";
import SettingsDrawer from "./components/SettingsDrawer";
import type {
  Agent,
  AgentStatus,
  ArchiveIntegrityReport,
  ChatMessage,
  CrystalBallSmokeResponse,
  CrystalBallValidateResponse,
  CrystalBallHealth,
  CrystalBallEvent,
  GateSnapshot,
  KaizenSettings,
  KaizenSettingsPatch,
} from "./types";

const initialKaizenMessages: ChatMessage[] = [
  {
    role: "kaizen",
    content: "Kaizen online. Ready to plan, reason, and review.",
    timestamp: new Date().toISOString(),
  },
];

function App() {
  const [settings, setSettings] = useState<KaizenSettings | null>(null);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [kaizenMessages, setKaizenMessages] =
    useState<ChatMessage[]>(initialKaizenMessages);
  const [gateSnapshot, setGateSnapshot] = useState<GateSnapshot | null>(null);
  const [events, setEvents] = useState<CrystalBallEvent[]>([]);
  const [crystalBallHealth, setCrystalBallHealth] =
    useState<CrystalBallHealth | null>(null);
  const [crystalBallAudit, setCrystalBallAudit] =
    useState<ArchiveIntegrityReport | null>(null);
  const [crystalBallValidation, setCrystalBallValidation] =
    useState<CrystalBallValidateResponse | null>(null);
  const [crystalBallSmoke, setCrystalBallSmoke] =
    useState<CrystalBallSmokeResponse | null>(null);
  const [crystalBallOpen, setCrystalBallOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [sendingKaizen, setSendingKaizen] = useState(false);
  const [streamingContent, setStreamingContent] = useState("");
  const [error, setError] = useState<string | null>(null);

  const gateState = gateSnapshot?.current_state ?? "plan";
  const maxSubagents = settings?.max_subagents ?? 5;

  useEffect(() => {
    const load = async () => {
      setError(null);
      try {
        const loadedSettings = await fetchSettings();
        setSettings(loadedSettings);

        const defaultOpen = loadedSettings.new_agent_chat_default_state === "open";
        const [loadedAgents, loadedGates] = await Promise.all([
          fetchAgents(defaultOpen),
          fetchGateSnapshot(),
        ]);
        setAgents(loadedAgents);
        setGateSnapshot(loadedGates);

        if (loadedSettings.crystal_ball_enabled) {
          setCrystalBallOpen(loadedSettings.crystal_ball_default_open);
          const [loadedEvents, loadedHealth, loadedAudit] = await Promise.all([
            fetchCrystalBallEvents(100),
            fetchCrystalBallHealth(),
            fetchCrystalBallAudit(),
          ]);
          setEvents(loadedEvents);
          setCrystalBallHealth(loadedHealth);
          setCrystalBallAudit(loadedAudit);
          setCrystalBallValidation(null);
          setCrystalBallSmoke(null);
        }
      } catch (loadError) {
        setError(loadError instanceof Error ? loadError.message : "Failed to load app state");
      }
    };

    void load();
  }, []);

  useEffect(() => {
    if (!settings?.crystal_ball_enabled) {
      setEvents([]);
      setCrystalBallHealth(null);
      setCrystalBallAudit(null);
      setCrystalBallValidation(null);
      setCrystalBallSmoke(null);
      return;
    }

    const poll = async () => {
      try {
        const [loadedEvents, loadedHealth] = await Promise.all([
          fetchCrystalBallEvents(150),
          fetchCrystalBallHealth(),
        ]);
        setEvents(loadedEvents);
        setCrystalBallHealth(loadedHealth);
      } catch {
        // Keep current events if polling fails.
      }
    };

    const timer = window.setInterval(() => {
      void poll();
    }, 2000);

    return () => window.clearInterval(timer);
  }, [settings?.crystal_ball_enabled]);

  const openAgentChats = useMemo(
    () => agents.filter((agent) => agent.chatOpen).length,
    [agents]
  );

  const patchLocalAgent = (agentId: string, patch: Partial<Agent>) => {
    setAgents((current) =>
      current.map((agent) =>
        agent.id === agentId ? { ...agent, ...patch } : agent
      )
    );
  };

  const appendAgentMessage = (agentId: string, message: ChatMessage) => {
    setAgents((current) =>
      current.map((agent) =>
        agent.id === agentId
          ? { ...agent, messages: [...agent.messages, message] }
          : agent
      )
    );
  };

  const toggleAgentChat = (agentId: string) => {
    setAgents((current) =>
      current.map((agent) =>
        agent.id === agentId ? { ...agent, chatOpen: !agent.chatOpen } : agent
      )
    );
  };

  const handleSendKaizen = async (message: string) => {
    setSendingKaizen(true);
    setStreamingContent("");
    setError(null);

    setKaizenMessages((current) => [
      ...current,
      {
        role: "user",
        content: message,
        timestamp: new Date().toISOString(),
      },
    ]);

    // Try streaming first, fall back to non-streaming
    try {
      await new Promise<void>((resolve, reject) => {
        streamChat(
          message,
          {
            onToken: (token) => {
              setStreamingContent((prev) => prev + token.text);
            },
            onDone: (done) => {
              // Move streaming content to permanent messages
              setStreamingContent("");
              setKaizenMessages((current) => [
                ...current,
                {
                  role: "kaizen",
                  content: done.full_response,
                  timestamp: new Date().toISOString(),
                },
              ]);
              resolve();
            },
            onError: (errMsg) => {
              reject(new Error(errMsg));
            },
          }
        );
      });
    } catch {
      // Streaming failed - fall back to non-streaming
      setStreamingContent("");
      try {
        const response = await sendChat(message);
        setKaizenMessages((current) => [
          ...current,
          {
            role: "kaizen",
            content: response.reply,
            timestamp: new Date().toISOString(),
          },
        ]);

        setGateSnapshot((current) =>
          current ? { ...current, current_state: response.gate_state } : current
        );
      } catch (sendError) {
        const text = sendError instanceof Error ? sendError.message : "Chat failed";
        setError(text);
        setKaizenMessages((current) => [
          ...current,
          {
            role: "kaizen",
            content: `Request failed: ${text}`,
            timestamp: new Date().toISOString(),
          },
        ]);
      }
    } finally {
      setSendingKaizen(false);
    }
  };

  const handleSpawnAgent = async (input: {
    agentName: string;
    taskId: string;
    objective: string;
  }) => {
    const defaultOpen = settings?.new_agent_chat_default_state === "open";
    const created = await createAgent(
      {
        agent_name: input.agentName,
        task_id: input.taskId,
        objective: input.objective,
      },
      defaultOpen
    );

    setAgents((current) => [
      ...current,
      {
        ...created,
        messages: [
          {
            role: "agent",
            content: `${created.name} online for ${created.taskId}.`,
            timestamp: new Date().toISOString(),
          },
        ],
      },
    ]);
  };

  const handleSetAgentStatus = async (
    agentId: string,
    status: AgentStatus,
    kaizenReviewApproved: boolean
  ) => {
    const updated = await updateAgentStatus(agentId, {
      status,
      kaizen_review_approved: kaizenReviewApproved,
    });

    patchLocalAgent(agentId, {
      status: updated.status,
      objective: updated.objective,
      taskId: updated.taskId,
    });
  };

  const handleRenameAgent = async (agentId: string, newName: string) => {
    const updated = await renameAgent(agentId, newName);
    patchLocalAgent(agentId, { name: updated.name });
  };

  const handleSendAgentMessage = async (agentId: string, message: string) => {
    appendAgentMessage(agentId, {
      role: "user",
      content: message,
      timestamp: new Date().toISOString(),
    });

    try {
      const response = await sendChat(message, agentId);
      appendAgentMessage(agentId, {
        role: "agent",
        content: response.reply,
        timestamp: new Date().toISOString(),
      });
    } catch (chatError) {
      appendAgentMessage(agentId, {
        role: "agent",
        content:
          chatError instanceof Error
            ? `Request failed: ${chatError.message}`
            : "Request failed.",
        timestamp: new Date().toISOString(),
      });
    }
  };

  const handleUpdateSettings = async (patch: KaizenSettingsPatch) => {
    const updated = await patchSettings(patch);
    setSettings(updated);

    if (!updated.crystal_ball_enabled) {
      setCrystalBallOpen(false);
      setEvents([]);
      setCrystalBallHealth(null);
      setCrystalBallAudit(null);
      setCrystalBallValidation(null);
      setCrystalBallSmoke(null);
    }

    if (
      Object.prototype.hasOwnProperty.call(patch, "crystal_ball_default_open") &&
      updated.crystal_ball_enabled
    ) {
      setCrystalBallOpen(updated.crystal_ball_default_open);
    }

    if (updated.crystal_ball_enabled) {
      const [loadedHealth, loadedAudit] = await Promise.all([
        fetchCrystalBallHealth(),
        fetchCrystalBallAudit(),
      ]);
      setCrystalBallHealth(loadedHealth);
      setCrystalBallAudit(loadedAudit);
      setCrystalBallValidation(null);
      setCrystalBallSmoke(null);
    }
  };

  const refreshCrystalBallAudit = async () => {
    const [health, audit] = await Promise.all([
      fetchCrystalBallHealth(),
      fetchCrystalBallAudit(),
    ]);
    setCrystalBallHealth(health);
    setCrystalBallAudit(audit);
  };

  const runCrystalBallValidation = async () => {
    const [validation, health] = await Promise.all([
      validateCrystalBall(),
      fetchCrystalBallHealth(),
    ]);
    setCrystalBallValidation(validation);
    setCrystalBallHealth(health);
  };

  const runCrystalBallSmokeTest = async () => {
    const smoke = await runCrystalBallSmoke();
    setCrystalBallSmoke(smoke);
    const [health, audit] = await Promise.all([
      fetchCrystalBallHealth(),
      fetchCrystalBallAudit(),
    ]);
    setCrystalBallHealth(health);
    setCrystalBallAudit(audit);
    if (settings?.crystal_ball_enabled) {
      const loadedEvents = await fetchCrystalBallEvents(150);
      setEvents(loadedEvents);
    }
  };

  const handleAdvanceGate = async () => {
    const transition = await advanceGate();
    setGateSnapshot((current) =>
      current ? { ...current, current_state: transition.to } : current
    );

    const refreshed = await fetchGateSnapshot();
    setGateSnapshot(refreshed);
  };

  const handlePatchGateCondition = async (
    key: keyof GateSnapshot["conditions"],
    value: boolean
  ) => {
    const snapshot = await patchGateConditions({ [key]: value });
    setGateSnapshot(snapshot);
  };

  return (
    <div className="app">
      <header className="app-header">
        <div>
          <h1>Kaizen MAX</h1>
          <p className="header-meta">
            Gate: {gateState} | Active agents: {agents.length} | Open chats: {openAgentChats}
          </p>
        </div>
        <nav className="app-nav">
          <button className="nav-btn" onClick={() => void handleAdvanceGate()}>
            Advance Gate
          </button>
          <button className="nav-btn" onClick={() => setSettingsOpen(true)}>
            Settings
          </button>
          <button
            className="nav-btn"
            disabled={!settings?.crystal_ball_enabled}
            onClick={() => setCrystalBallOpen(!crystalBallOpen)}
          >
            Crystal Ball {crystalBallOpen ? "(close)" : "(open)"}
          </button>
        </nav>
      </header>

      {error && <div className="error-banner">{error}</div>}

      <main className="app-main">
        <section className="chat-section">
          <KaizenChat
            messages={kaizenMessages}
            gateState={gateState}
            sending={sendingKaizen}
            streamingContent={streamingContent}
            onSend={handleSendKaizen}
          />

          {gateSnapshot && (
            <section className="gate-controls">
              <h3>Gate Conditions</h3>
              <div className="gate-toggle-grid">
                {Object.entries(gateSnapshot.conditions).map(([key, value]) => (
                  <label key={key}>
                    <input
                      type="checkbox"
                      checked={value}
                      onChange={(event) =>
                        void handlePatchGateCondition(
                          key as keyof GateSnapshot["conditions"],
                          event.target.checked
                        )
                      }
                    />
                    {key}
                  </label>
                ))}
              </div>
            </section>
          )}

          <AgentChats
            agents={agents}
            onClose={(agentId) => patchLocalAgent(agentId, { chatOpen: false })}
            onSend={handleSendAgentMessage}
          />
        </section>

        <aside className="agent-sidebar">
          <AgentPanel
            agents={agents}
            maxSubagents={maxSubagents}
            renameEnabled={settings?.agent_name_editable_after_spawn ?? true}
            onToggleChat={toggleAgentChat}
            onSpawn={handleSpawnAgent}
            onSetStatus={handleSetAgentStatus}
            onRename={handleRenameAgent}
          />
        </aside>
      </main>

      {settings?.crystal_ball_enabled && crystalBallOpen && (
        <CrystalBall events={events} onClose={() => setCrystalBallOpen(false)} />
      )}

      <SettingsDrawer
        isOpen={settingsOpen}
        settings={settings}
        crystalBallHealth={crystalBallHealth}
        crystalBallAudit={crystalBallAudit}
        crystalBallValidation={crystalBallValidation}
        crystalBallSmoke={crystalBallSmoke}
        onClose={() => setSettingsOpen(false)}
        onUpdate={handleUpdateSettings}
        onRefreshCrystalBallAudit={refreshCrystalBallAudit}
        onValidateCrystalBall={runCrystalBallValidation}
        onRunCrystalBallSmoke={runCrystalBallSmokeTest}
      />
    </div>
  );
}

export default App;
