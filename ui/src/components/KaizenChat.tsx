import { useState } from "react";

interface Message {
  role: "user" | "kaizen";
  content: string;
  timestamp: string;
}

/** Main Kaizen chat panel - always visible (main_chat_pinned: true) */
function KaizenChat() {
  const [messages, setMessages] = useState<Message[]>([
    {
      role: "kaizen",
      content: "Kaizen online. Ready to plan, reason, and review.",
      timestamp: new Date().toISOString(),
    },
  ]);
  const [input, setInput] = useState("");

  const handleSend = () => {
    if (!input.trim()) return;
    const userMsg: Message = {
      role: "user",
      content: input.trim(),
      timestamp: new Date().toISOString(),
    };
    setMessages((prev) => [...prev, userMsg]);
    setInput("");

    // TODO: Send to ZeroClaw gateway /api/chat and stream response
    const kaizenReply: Message = {
      role: "kaizen",
      content: "[Gateway integration pending - Phase C/D]",
      timestamp: new Date().toISOString(),
    };
    setMessages((prev) => [...prev, kaizenReply]);
  };

  return (
    <div className="kaizen-chat">
      <div className="chat-header">
        <h2>Kaizen</h2>
        <span className="status-badge">Primary Agent</span>
      </div>
      <div className="chat-messages">
        {messages.map((msg, i) => (
          <div key={i} className={`message message-${msg.role}`}>
            <span className="message-role">
              {msg.role === "kaizen" ? "Kaizen" : "You"}
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
          onKeyDown={(e) => e.key === "Enter" && handleSend()}
          placeholder="Talk to Kaizen..."
        />
        <button onClick={handleSend}>Send</button>
      </div>
    </div>
  );
}

export default KaizenChat;
