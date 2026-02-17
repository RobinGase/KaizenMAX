import { useEffect, useRef, useState } from "react";
import type { ChatMessage, GateState } from "../types";

interface KaizenChatProps {
  messages: ChatMessage[];
  gateState: GateState;
  sending: boolean;
  streamingContent: string;
  onSend: (message: string) => Promise<void>;
}

/** Main Kaizen chat panel - always visible (main_chat_pinned: true) */
function KaizenChat({
  messages,
  gateState,
  sending,
  streamingContent,
  onSend,
}: KaizenChatProps) {
  const [input, setInput] = useState("");
  const messagesEndRef = useRef<HTMLDivElement>(null);

  const handleSend = async () => {
    const content = input.trim();
    if (!content || sending) return;
    setInput("");
    await onSend(content);
  };

  // Auto-scroll to bottom on new messages or streaming content
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, streamingContent]);

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
              {msg.role === "kaizen"
                ? "Kaizen"
                : msg.role === "agent"
                  ? "Agent"
                  : "You"}
            </span>
            <div className="message-content">{msg.content}</div>
          </div>
        ))}
        {streamingContent && (
          <div className="message message-kaizen message-streaming">
            <span className="message-role">Kaizen</span>
            <div className="message-content">
              {streamingContent}
              <span className="streaming-cursor" />
            </div>
          </div>
        )}
        <div ref={messagesEndRef} />
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
          disabled={sending}
        />
        <button onClick={() => void handleSend()} disabled={sending}>
          {sending ? "Thinking..." : "Send"}
        </button>
      </div>
    </div>
  );
}

export default KaizenChat;
