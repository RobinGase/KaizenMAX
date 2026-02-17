import { useMemo, useState } from "react";
import type { Agent } from "../types";

interface AgentChatsProps {
  agents: Agent[];
  onClose: (agentId: string) => void;
  onSend: (agentId: string, message: string) => Promise<void>;
}

function AgentChats({ agents, onClose, onSend }: AgentChatsProps) {
  const [inputs, setInputs] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<Record<string, boolean>>({});

  const openAgents = useMemo(
    () => agents.filter((agent) => agent.chatOpen),
    [agents]
  );

  if (openAgents.length === 0) {
    return null;
  }

  return (
    <div className="agent-chats">
      {openAgents.map((agent) => {
        const value = inputs[agent.id] ?? "";
        const isBusy = busy[agent.id] ?? false;

        const send = async () => {
          const message = value.trim();
          if (!message || isBusy) return;

          setBusy((prev) => ({ ...prev, [agent.id]: true }));
          setInputs((prev) => ({ ...prev, [agent.id]: "" }));
          try {
            await onSend(agent.id, message);
          } finally {
            setBusy((prev) => ({ ...prev, [agent.id]: false }));
          }
        };

        return (
          <section className="agent-chat-window" key={agent.id}>
            <header className="agent-chat-header">
              <h4>{agent.name}</h4>
              <span>{agent.status}</span>
              <button onClick={() => onClose(agent.id)} type="button">
                Close
              </button>
            </header>

            <div className="agent-chat-messages">
              {agent.messages.length === 0 ? (
                <p className="agent-chat-empty">No chat yet. Send an instruction.</p>
              ) : (
                agent.messages.map((message, index) => (
                  <div
                    className={`message message-${message.role}`}
                    key={`${agent.id}-${index}`}
                  >
                    <span className="message-role">
                      {message.role === "agent" ? agent.name : "You"}
                    </span>
                    <p>{message.content}</p>
                  </div>
                ))
              )}
            </div>

            <div className="agent-chat-input">
              <input
                value={value}
                onChange={(event) =>
                  setInputs((prev) => ({ ...prev, [agent.id]: event.target.value }))
                }
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    void send();
                  }
                }}
                placeholder={`Message ${agent.name}...`}
              />
              <button onClick={() => void send()} disabled={isBusy} type="button">
                {isBusy ? "Sending..." : "Send"}
              </button>
            </div>
          </section>
        );
      })}
    </div>
  );
}

export default AgentChats;
