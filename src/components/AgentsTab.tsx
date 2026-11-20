import React from "react";
import { Bot, Terminal } from "lucide-react";
import { AgentSessionState } from "../types";

interface Props {
  agents: AgentSessionState[];
}

export const AgentsTab: React.FC<Props> = ({ agents }) => {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div className="glass-card row-between">
        <div className="row">
          <Terminal size={15} />
          <div>
            <div style={{ fontSize: 12, fontWeight: 600 }}>Local Agent Loopback API</div>
            <div className="muted mono">POST http://127.0.0.1:47823/notify · /tile</div>
          </div>
        </div>
        <span className="badge">LISTENING</span>
      </div>

      {agents.length === 0 ? (
        <div className="glass-card" style={{ padding: 18 }}>
          <div style={{ fontWeight: 600, fontSize: 13, marginBottom: 6 }}>
            Monitor Claude Code, Codex, Antigravity, Hermes & Custom Agents
          </div>
          <div className="muted mono" style={{ fontSize: 11, lineHeight: 1.5 }}>
            notch-cli notify --agent claude-code --status needs_input --title &quot;Approve Edit&quot;
          </div>
        </div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          {agents.map((a) => (
            <div key={a.agent} className="glass-card row-between">
              <div className="row" style={{ minWidth: 0 }}>
                <Bot size={16} />
                <div style={{ minWidth: 0 }}>
                  <div style={{ fontSize: 13, fontWeight: 600 }}>
                    {a.agent} — {a.title}
                  </div>
                  <div className="muted">{a.body || "Active session"}</div>
                </div>
              </div>
              <span className="badge" style={{ textTransform: "uppercase" }}>
                {a.status}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
};
