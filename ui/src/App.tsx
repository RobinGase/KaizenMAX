import { useState } from "react";
import KaizenChat from "./components/KaizenChat";
import AgentPanel from "./components/AgentPanel";
import CrystalBall from "./components/CrystalBall";
import type { Agent } from "./types";

function App() {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [crystalBallOpen, setCrystalBallOpen] = useState(false);

  return (
    <div className="app">
      <header className="app-header">
        <h1>Kaizen MAX</h1>
        <nav className="app-nav">
          <button
            className="nav-btn"
            onClick={() => setCrystalBallOpen(!crystalBallOpen)}
          >
            Crystal Ball {crystalBallOpen ? "(close)" : "(open)"}
          </button>
        </nav>
      </header>

      <main className="app-main">
        <section className="chat-section">
          <KaizenChat />
        </section>

        <aside className="agent-sidebar">
          <AgentPanel agents={agents} onAgentsChange={setAgents} />
        </aside>
      </main>

      {crystalBallOpen && (
        <CrystalBall onClose={() => setCrystalBallOpen(false)} />
      )}
    </div>
  );
}

export default App;
