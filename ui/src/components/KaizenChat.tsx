import { useState } from "react";
import type { ChatMessage, GateState } from "../types";

interface KaizenChatProps {
  messages: ChatMessage[];
  gateState: GateState;
  sending: boolean;
  onSend: (message: string) => Promise<void>;
}

/** Main Kaizen chat panel - always visible (main_chat_pinned: true) */
function KaizenChat({ messages, gateState, sending, onSend }: KaizenChatProps) {
  const [input, setInput] = useState("");

  const handleSend = async () => {
    const content = input.trim();
    if (!content || sending) return;
    setInput("");
    await onSend(content);
  };

  return (
    <div className="kaizen-chat">
      <div className="chat-header">
        <h2>Kaizen</h2>
        <span className="status-badge">Primary Agent</span>
        <span className="gate-badge">Gate: {gateState}</span>
      </div>
      <div className="chat-messages">
        {messages.map((msg, i) => (
          <div key={i} className={`message message-${msg.role}`}>
            <span className="message-role">
              {msg.role === "kaizen" ? "Kaizen" : msg.role === "agent" ? "Agent" : "You"}
            </span>
            <p>{msg.content}</p>
          </div>
        ))}
      </div>
      <div className="chat-input">
        <input
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              void handleSend();
            }
          }}
          placeholder="Talk to Kaizen..."
        />
        <button onClick={() => void handleSend()} disabled={sending}>
          {sending ? "Sending..." : "Send"}
        </button>
      </div>
    </div>
  );
}

export default KaizenChat;
