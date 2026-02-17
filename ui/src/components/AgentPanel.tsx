import { useMemo, useState } from "react";
import type { Agent } from "../types";
import type { AgentStatus } from "../types";

interface AgentPanelProps {
  agents: Agent[];
  maxSubagents: number;
  renameEnabled: boolean;
  onToggleChat: (agentId: string) => void;
  onSpawn: (input: {
    agentName: string;
    taskId: string;
    objective: string;
  }) => Promise<void>;
  onSetStatus: (
    agentId: string,
    status: AgentStatus,
    kaizenReviewApproved: boolean
  ) => Promise<void>;
  onRename: (agentId: string, newName: string) => Promise<void>;
}

/** Sidebar showing all agents and their status. Click to toggle chat. */
function AgentPanel({
  agents,
  maxSubagents,
  renameEnabled,
  onToggleChat,
  onSpawn,
  onSetStatus,
  onRename,
}: AgentPanelProps) {
  const [agentName, setAgentName] = useState("Builder-1");
  const [taskId, setTaskId] = useState("task-001");
  const [objective, setObjective] = useState("Implement requested feature slice");
  const [spawning, setSpawning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");

  const activeCount = useMemo(
    () => agents.filter((agent) => agent.status !== "done").length,
    [agents]
  );

  const canSpawn = activeCount < maxSubagents;

  const handleSpawn = async () => {
    if (!agentName.trim() || !taskId.trim() || !objective.trim()) {
      setError("Agent name, task ID, and objective are required.");
      return;
    }

    setError(null);
    setSpawning(true);
    try {
      await onSpawn({
        agentName: agentName.trim(),
        taskId: taskId.trim(),
        objective: objective.trim(),
      });
      setAgentName((name) => {
        const index = Number(name.match(/(\d+)$/)?.[1] ?? "1") + 1;
        return `Builder-${index}`;
      });
      setTaskId((id) => {
        const index = Number(id.match(/(\d+)$/)?.[1] ?? "1") + 1;
        return `task-${String(index).padStart(3, "0")}`;
      });
    } catch (spawnError) {
      setError(
        spawnError instanceof Error ? spawnError.message : "Failed to spawn agent."
      );
    } finally {
      setSpawning(false);
    }
  };

  const handleSetStatus = async (
    agentId: string,
    status: AgentStatus,
    kaizenReviewApproved: boolean
  ) => {
    setError(null);
    try {
      await onSetStatus(agentId, status, kaizenReviewApproved);
    } catch (statusError) {
      setError(
        statusError instanceof Error
          ? statusError.message
          : "Failed to update agent status."
      );
    }
  };

  const startRename = (agent: Agent) => {
    if (!renameEnabled) return;
    setEditingId(agent.id);
    setEditingName(agent.name);
  };

  const confirmRename = async (agentId: string) => {
    const trimmed = editingName.trim();
    if (!trimmed) {
      setEditingId(null);
      return;
    }
    setError(null);
    try {
      await onRename(agentId, trimmed);
    } catch (renameError) {
      setError(
        renameError instanceof Error
          ? renameError.message
          : "Failed to rename agent."
      );
    }
    setEditingId(null);
  };

  const cancelRename = () => {
    setEditingId(null);
    setEditingName("");
  };

  return (
    <div className="agent-panel">
      <h3>Agents</h3>
      <p className="agent-capacity">
        Active: {activeCount} / {maxSubagents}
      </p>

      <div className="agent-spawn-form">
        <input
          value={agentName}
          onChange={(e) => setAgentName(e.target.value)}
          placeholder="Agent name"
        />
        <input
          value={taskId}
          onChange={(e) => setTaskId(e.target.value)}
          placeholder="Task ID"
        />
        <textarea
          value={objective}
          onChange={(e) => setObjective(e.target.value)}
          placeholder="Objective"
          rows={2}
        />
        <button disabled={!canSpawn || spawning} onClick={() => void handleSpawn()}>
          {spawning ? "Spawning..." : "Spawn Agent"}
        </button>
      </div>
      {error && <p className="agent-error">{error}</p>}

      {agents.length === 0 ? (
        <p className="agent-empty">
          No sub-agents active. Ask Kaizen to spawn agents when needed.
        </p>
      ) : (
        <ul className="agent-list">
          {agents.map((agent) => (
            <li key={agent.id} className={`agent-item agent-${agent.status}`}>
              {editingId === agent.id ? (
                <input
                  className="agent-rename-input"
                  value={editingName}
                  onChange={(e) => setEditingName(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") void confirmRename(agent.id);
                    if (e.key === "Escape") cancelRename();
                  }}
                  onBlur={() => void confirmRename(agent.id)}
                  autoFocus
                />
              ) : (
                <span
                  className={`agent-name ${renameEnabled ? "agent-name-editable" : ""}`}
                  onClick={() => startRename(agent)}
                  title={renameEnabled ? "Click to rename" : ""}
                >
                  {agent.name}
                </span>
              )}
              <span className="agent-status">{agent.status}</span>
              <button
                className="agent-toggle"
                onClick={() => onToggleChat(agent.id)}
                type="button"
              >
                {agent.chatOpen ? "Hide" : "Chat"}
              </button>
              <div className="agent-actions">
                <button
                  type="button"
                  onClick={() => void handleSetStatus(agent.id, "active", false)}
                >
                  Active
                </button>
                <button
                  type="button"
                  onClick={() =>
                    void handleSetStatus(agent.id, "review_pending", false)
                  }
                >
                  Review
                </button>
                <button
                  type="button"
                  onClick={() => void handleSetStatus(agent.id, "done", true)}
                >
                  Done
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export default AgentPanel;
