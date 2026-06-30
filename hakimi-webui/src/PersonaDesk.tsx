import type { DeskState } from './officeState';
import { displayedState } from './officeState';

interface PersonaDeskProps {
  desk: DeskState;
  x: number;
  y: number;
  onOpen: (id: string) => void;
  onHover: (id: string | null) => void;
}

const ROLE_COLORS: Record<string, string> = {
  code: '#2563eb',
  ops: '#d97706',
  research: '#7c3aed',
  security: '#dc2626',
  docs: '#059669',
  default: '#475569',
};

const ROLE_LABELS: Record<string, string> = {
  code: 'Code',
  ops: 'Ops',
  research: 'Research',
  security: 'Security',
  docs: 'Docs',
  default: 'General',
};

function inferRole(desk: DeskState): string {
  const text = `${desk.id} ${desk.name}`.toLowerCase();
  if (/code|coding|代码|编程|开发/.test(text)) return 'code';
  if (/ops|运维|deploy|部署|devops|monitor/.test(text)) return 'ops';
  if (/research|调研|分析|analyst/.test(text)) return 'research';
  if (/secur|安全|audit|审计/.test(text)) return 'security';
  if (/doc|文档|write|writing|知识/.test(text)) return 'docs';
  return 'default';
}

function avatarText(desk: DeskState): string {
  if (desk.avatar.trim()) return desk.avatar.trim().slice(0, 2);
  return (desk.name.trim() || desk.id).slice(0, 1).toUpperCase();
}

export default function PersonaDesk({ desk, x, y, onOpen, onHover }: PersonaDeskProps) {
  const state = displayedState(desk);
  const working = state === 'working';
  const idle = state === 'idle';
  const dimmed = state === 'in_team';
  const role = inferRole(desk);
  const color = ROLE_COLORS[role];
  const label = ROLE_LABELS[role];

  return (
    <div
      className={`persona-desk ${dimmed ? 'is-dimmed' : ''} ${working ? 'is-working' : ''}`}
      style={{ left: x, top: y, '--desk-color': color } as React.CSSProperties}
      role="button"
      tabIndex={0}
      title={`${desk.name || desk.id} - ${state}`}
      onClick={() => onOpen(desk.id)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onOpen(desk.id);
        }
      }}
      onMouseEnter={() => onHover(desk.id)}
      onMouseLeave={() => onHover(null)}
    >
      <svg viewBox="0 0 180 150" width="180" height="150" aria-hidden="true">
        {/* Ground shadow */}
        <ellipse cx="90" cy="144" rx="42" ry="4" fill="rgba(0,0,0,0.06)" />

        {/* Desk surface */}
        <rect x="22" y="112" width="136" height="8" rx="3" fill={color} fillOpacity={0.15} stroke={color} strokeWidth="0.8" strokeOpacity={0.25} />

        {/* Laptop base */}
        <rect x="58" y="108" width="64" height="6" rx="2" fill="#c4b896" fillOpacity={0.5} />

        {/* Laptop screen */}
        <rect
          className={working ? 'screen-lit' : idle ? 'screen-game' : 'screen-off'}
          x="60" y="78" width="60" height="32" rx="3"
        />

        {/* Screen content */}
        {working && (
          <g className="desk-screen-lines">
            <rect x="66" y="85" width="24" height="2" rx="1" fill="white" fillOpacity={0.5} />
            <rect x="66" y="90" width="16" height="2" rx="1" fill="white" fillOpacity={0.35} />
            <rect x="66" y="95" width="28" height="2" rx="1" fill="white" fillOpacity={0.25} />
            <rect x="66" y="100" width="12" height="2" rx="1" fill="white" fillOpacity={0.2} />
          </g>
        )}
        {idle && (
          <>
            <rect className="game-block" x="72" y="86" width="8" height="8" rx="2" fill="#ffd166" />
            <rect className="game-block b2" x="88" y="94" width="8" height="8" rx="2" fill="#06d6a0" />
          </>
        )}

        {/* Body / shirt */}
        <path
          d="M62 80 C62 66 72 60 90 60 C108 60 118 66 118 80 L118 108 C118 110 116 112 114 112 L66 112 C64 112 62 110 62 108 Z"
          fill={color}
          fillOpacity={0.12}
          stroke={color}
          strokeWidth="1"
          strokeOpacity={0.3}
        />

        {/* Collar V */}
        <path d="M80 60 L90 72 L100 60" fill="none" stroke={color} strokeWidth="1" strokeOpacity={0.3} />

        {/* Head */}
        <circle cx="90" cy="38" r="20" fill="#fef3e2" stroke={color} strokeWidth="1.2" strokeOpacity={0.4} />

        {/* Hair */}
        <path
          d="M70 34 C70 20 78 14 90 14 C102 14 110 20 110 34"
          fill={color}
          fillOpacity={0.18}
          stroke={color}
          strokeWidth="1"
          strokeOpacity={0.35}
        />

        {/* Eyes */}
        <g className="desk-eyes">
          <ellipse cx="82" cy="38" rx="2.4" ry="2.8" fill={color} fillOpacity={0.7} />
          <ellipse cx="98" cy="38" rx="2.4" ry="2.8" fill={color} fillOpacity={0.7} />
          <circle cx="83.2" cy="37" r="0.9" fill="white" />
          <circle cx="99.2" cy="37" r="0.9" fill="white" />
        </g>

        {/* Mouth */}
        {working ? (
          <ellipse className="desk-mouth-work" cx="90" cy="47" rx="3.5" ry="2" fill={color} fillOpacity={0.2} />
        ) : (
          <path d="M84 46 Q90 50 96 46" fill="none" stroke={color} strokeWidth="1" strokeOpacity={0.35} />
        )}

        {/* Arms */}
        <g className={working ? 'desk-arms-typing' : ''}>
          <path d="M62 80 L46 94 L50 108" fill="none" stroke={color} strokeWidth="1.5" strokeLinecap="round" strokeOpacity={0.4} />
          <path d="M118 80 L134 94 L130 108" fill="none" stroke={color} strokeWidth="1.5" strokeLinecap="round" strokeOpacity={0.4} />
          {/* Hands */}
          <circle cx="50" cy="109" r="4" fill="#fef3e2" stroke={color} strokeWidth="0.8" strokeOpacity={0.35} />
          <circle cx="130" cy="109" r="4" fill="#fef3e2" stroke={color} strokeWidth="0.8" strokeOpacity={0.35} />
        </g>

        {/* Working: thinking bubbles */}
        {working && (
          <g className="desk-think-dots">
            <circle cx="128" cy="28" r="2.5" fill={color} fillOpacity={0.5}>
              <animate attributeName="fill-opacity" values="0.5;0.15;0.5" dur="1.4s" repeatCount="indefinite" />
            </circle>
            <circle cx="136" cy="20" r="3.5" fill={color} fillOpacity={0.35}>
              <animate attributeName="fill-opacity" values="0.35;0.1;0.35" dur="1.4s" begin="0.35s" repeatCount="indefinite" />
            </circle>
            <circle cx="146" cy="12" r="4.5" fill={color} fillOpacity={0.25}>
              <animate attributeName="fill-opacity" values="0.25;0.06;0.25" dur="1.4s" begin="0.7s" repeatCount="indefinite" />
            </circle>
          </g>
        )}

        {/* Avatar badge on shirt */}
        <circle cx="90" cy="88" r="10" fill={color} fillOpacity={0.15} stroke={color} strokeWidth="0.8" strokeOpacity={0.3} />
        <text x="90" y="92" textAnchor="middle" fontSize="10" fill={color} fillOpacity={0.6}>{avatarText(desk)}</text>
      </svg>

      <div className="desk-label">
        <span className="desk-name">{desk.name || desk.id}</span>
        <span className="desk-role" style={{ color }}>{label}</span>
      </div>
    </div>
  );
}
