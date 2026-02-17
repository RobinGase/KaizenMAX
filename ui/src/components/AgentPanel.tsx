import type { Agent } from "../types";

interface AgentPanelProps {
  agents: Agent[];
  onAgentsChange: (agents: Agent[]) => void;
}

/** Sidebar showing all agents and their status. Click to toggle chat. */
function AgentPanel({ agents, onAgentsChange }: AgentPanelProps) {
  const toggleChat = (agentId: string) => {
    onAgentsChange(
      agents.map((a) =>
        a.id === agentId ? { ...a, chatOpen: !a.chatOpen } : a
      )
    );
  };

  return (
    <div className="agent-panel">
      <h3>Agents</h3>
      {agents.length === 0 ? (
        <p className="agent-empty">
          No sub-agents active. Ask Kaizen to spawn agents when needed.
        </p>
      ) : (
        <ul className="agent-list">
          {agents.map((agent) => (
            <li
              key={agent.id}
              className={`agent-item agent-${agent.status}`}
              onClick={() => toggleChat(agent.id)}
            >
              <span className="agent-name">{agent.name}</span>
              <span className="agent-status">{agent.status}</span>
              <span className="agent-toggle">
                {agent.chatOpen ? "[-]" : "[+]"}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export default AgentPanel;
