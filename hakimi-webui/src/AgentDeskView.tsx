import type { Agent } from './api';

interface AgentDeskViewProps {
  agents: Agent[];
  activeId: string | null;
  sending: boolean;
  onSelect: (id: string) => void;
}

const ROLE_META: Record<string, { icon: string; color: string; domain: string }> = {
  code: { icon: 'code', color: '#2563eb', domain: 'Code & Architecture' },
  ops: { icon: 'ops', color: '#d97706', domain: 'Deploy & Monitoring' },
  research: { icon: 'research', color: '#7c3aed', domain: 'Analysis & Research' },
  security: { icon: 'security', color: '#dc2626', domain: 'Security & Compliance' },
  docs: { icon: 'docs', color: '#059669', domain: 'Docs & Knowledge' },
  default: { icon: 'default', color: '#475569', domain: 'General Purpose' },
};

function inferRole(agent: Agent): keyof typeof ROLE_META {
  const text = `${agent.id} ${agent.name} ${agent.description}`.toLowerCase();
  if (/code|coding|代码|编程|开发/.test(text)) return 'code';
  if (/ops|运维|deploy|部署|devops|monitor/.test(text)) return 'ops';
  if (/research|调研|分析|research|analyst/.test(text)) return 'research';
  if (/secur|安全|audit|审计/.test(text)) return 'security';
  if (/doc|文档|write|writing|知识/.test(text)) return 'docs';
  return 'default';
}

function RoleIcon({ role, size = 20 }: { role: string; size?: number }) {
  const s = size;
  const half = s / 2;
  const common = { xmlns: 'http://www.w3.org/2000/svg', width: s, height: s, viewBox: `0 0 ${s} ${s}`, fill: 'none', stroke: 'currentColor', strokeWidth: 1.6, strokeLinecap: 'round' as const, strokeLinejoin: 'round' as const };

  if (role === 'code') {
    return (
      <svg {...common}>
        <polyline points={`${half - 4},${half - 4} ${half - 8},${half} ${half - 4},${half + 4}`} />
        <polyline points={`${half + 4},${half - 4} ${half + 8},${half} ${half + 4},${half + 4}`} />
      </svg>
    );
  }
  if (role === 'ops') {
    return (
      <svg {...common}>
        <circle cx={half} cy={half} r={6} />
        <path d={`M${half},${half - 6}v-2 M${half},${half + 6}v2 M${half - 6},${half}h-2 M${half + 6},${half}h2`} />
      </svg>
    );
  }
  if (role === 'research') {
    return (
      <svg {...common}>
        <circle cx={half - 2} cy={half - 2} r={5} />
        <line x1={half + 2} y1={half + 2} x2={half + 6} y2={half + 6} />
      </svg>
    );
  }
  if (role === 'security') {
    return (
      <svg {...common}>
        <path d={`M${half},${half - 7} L${half + 6},${half - 4} L${half + 6},${half + 2} C${half + 6},${half + 6} ${half},${half + 8} ${half},${half + 8} C${half},${half + 8} ${half - 6},${half + 6} ${half - 6},${half + 2} L${half - 6},${half - 4} Z`} />
      </svg>
    );
  }
  if (role === 'docs') {
    return (
      <svg {...common}>
        <rect x={half - 5} y={half - 7} width={10} height={14} rx={1} />
        <line x1={half - 3} y1={half - 3} x2={half + 3} y2={half - 3} />
        <line x1={half - 3} y1={half} x2={half + 3} y2={half} />
        <line x1={half - 3} y1={half + 3} x2={half + 1} y2={half + 3} />
      </svg>
    );
  }
  // default: bot
  return (
    <svg {...common}>
      <rect x={half - 6} y={half - 4} width={12} height={10} rx={2} />
      <circle cx={half - 3} cy={half} r={1.2} fill="currentColor" stroke="none" />
      <circle cx={half + 3} cy={half} r={1.2} fill="currentColor" stroke="none" />
      <line x1={half} y1={half - 4} x2={half} y2={half - 7} />
      <circle cx={half} cy={half - 8} r={1} />
    </svg>
  );
}

function HumanFigure({ working, color }: { role: string; working: boolean; color: string }) {
  return (
    <svg className={`desk-figure ${working ? 'is-working' : ''}`} viewBox="0 0 120 140" fill="none" xmlns="http://www.w3.org/2000/svg">
      {/* Shadow */}
      <ellipse cx="60" cy="134" rx="28" ry="4" fill="rgba(0,0,0,0.06)" />

      {/* Body */}
      <path
        d="M40 90 C40 78 48 72 60 72 C72 72 80 78 80 90 L80 110 C80 114 77 116 74 116 L46 116 C43 116 40 114 40 110 Z"
        fill={color}
        opacity={0.15}
        stroke={color}
        strokeWidth="1.5"
      />

      {/* Shirt collar detail */}
      <path d="M52 72 L60 82 L68 72" fill="none" stroke={color} strokeWidth="1.2" opacity="0.4" />

      {/* Head */}
      <circle cx="60" cy="52" r="18" fill="#fef3e2" stroke={color} strokeWidth="1.5" />

      {/* Hair */}
      <path
        d="M42 48 C42 34 50 30 60 30 C70 30 78 34 78 48"
        fill={color}
        opacity="0.2"
        stroke={color}
        strokeWidth="1.2"
      />

      {/* Eyes */}
      <g className="desk-figure-eyes">
        <ellipse cx="52" cy="52" rx="2.2" ry="2.5" fill={color} />
        <ellipse cx="68" cy="52" rx="2.2" ry="2.5" fill={color} />
        {/* Eye glints */}
        <circle cx="53" cy="51" r="0.8" fill="white" />
        <circle cx="69" cy="51" r="0.8" fill="white" />
      </g>

      {/* Mouth - changes with working state */}
      {working ? (
        <g className="desk-figure-mouth-working">
          <ellipse cx="60" cy="60" rx="3" ry="2" fill={color} opacity="0.3" />
        </g>
      ) : (
        <path d="M55 59 Q60 63 65 59" fill="none" stroke={color} strokeWidth="1.2" opacity="0.5" />
      )}

      {/* Arms */}
      <g className={working ? 'desk-figure-arms-typing' : ''}>
        <path d="M40 90 L28 100 L32 110" fill="none" stroke={color} strokeWidth="1.5" strokeLinecap="round" opacity="0.6" />
        <path d="M80 90 L92 100 L88 110" fill="none" stroke={color} strokeWidth="1.5" strokeLinecap="round" opacity="0.6" />
        {/* Hands */}
        <circle cx="32" cy="111" r="3" fill="#fef3e2" stroke={color} strokeWidth="1" opacity="0.6" />
        <circle cx="88" cy="111" r="3" fill="#fef3e2" stroke={color} strokeWidth="1" opacity="0.6" />
      </g>

      {/* Desk surface */}
      <rect x="18" y="114" width="84" height="6" rx="2" fill={color} fillOpacity={0.12} stroke={color} strokeWidth="0.8" strokeOpacity={0.25} />

      {/* Laptop on desk */}
      <g className={working ? 'desk-laptop-glow' : ''}>
        {/* Screen */}
        <rect x="42" y="100" width="36" height="14" rx="2" fill={working ? color : '#e2e8f0'} fillOpacity={working ? 0.2 : 0.5} stroke={color} strokeWidth="0.8" strokeOpacity={0.3} />
        {/* Screen content lines */}
        {working && (
          <g className="desk-screen-lines">
            <rect x="46" y="104" width="16" height="1.5" rx="0.5" fill={color} opacity="0.3" />
            <rect x="46" y="108" width="10" height="1.5" rx="0.5" fill={color} opacity="0.2" />
          </g>
        )}
      </g>

      {/* Working indicator: floating dots */}
      {working && (
        <g className="desk-thinking-dots">
          <circle cx="90" cy="44" r="2" fill={color} opacity="0.6">
            <animate attributeName="opacity" values="0.6;0.2;0.6" dur="1.2s" repeatCount="indefinite" />
          </circle>
          <circle cx="96" cy="38" r="2.5" fill={color} opacity="0.4">
            <animate attributeName="opacity" values="0.4;0.1;0.4" dur="1.2s" begin="0.3s" repeatCount="indefinite" />
          </circle>
          <circle cx="103" cy="32" r="3" fill={color} opacity="0.3">
            <animate attributeName="opacity" values="0.3;0.08;0.3" dur="1.2s" begin="0.6s" repeatCount="indefinite" />
          </circle>
        </g>
      )}
    </svg>
  );
}

export default function AgentDeskView({ agents, activeId, sending, onSelect }: AgentDeskViewProps) {
  if (agents.length === 0) return null;

  return (
    <div className="desk-view">
      <div className="desk-view-header">
        <p className="eyebrow">Live Agents</p>
        <h2>Click an agent to start a conversation</h2>
      </div>

      <div className="desk-grid">
        {agents.map((agent) => {
          const role = inferRole(agent);
          const meta = ROLE_META[role];
          const isActive = agent.id === activeId;
          const isWorking = isActive && sending;

          return (
            <button
              key={agent.id}
              type="button"
              className={`desk-card ${isActive ? 'is-active' : ''} ${isWorking ? 'is-working' : ''}`}
              onClick={() => onSelect(agent.id)}
              style={{ '--agent-color': meta.color } as React.CSSProperties}
            >
              <div className="desk-card-figure">
                <HumanFigure role={role} working={isWorking} color={meta.color} />
              </div>

              <div className="desk-card-info">
                <div className="desk-card-name">
                  {agent.avatar && <span className="desk-card-avatar">{agent.avatar}</span>}
                  <span>{agent.name || agent.id}</span>
                  {agent.is_default && <span className="desk-card-default">default</span>}
                </div>

                <div className="desk-card-status">
                  <span className={`desk-status-dot ${isWorking ? 'is-busy' : 'is-idle'}`} />
                  <span>{isWorking ? 'Working...' : 'Idle'}</span>
                </div>

                <div className="desk-card-domain">
                  <RoleIcon role={role} size={14} />
                  <span>{meta.domain}</span>
                </div>

                {agent.description && (
                  <p className="desk-card-desc">{agent.description}</p>
                )}

                {agent.enabled_skills.length > 0 && (
                  <div className="desk-card-skills">
                    {agent.enabled_skills.slice(0, 3).map((skill) => (
                      <span key={skill} className="desk-skill-tag">{skill}</span>
                    ))}
                    {agent.enabled_skills.length > 3 && (
                      <span className="desk-skill-tag desk-skill-more">+{agent.enabled_skills.length - 3}</span>
                    )}
                  </div>
                )}
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}
