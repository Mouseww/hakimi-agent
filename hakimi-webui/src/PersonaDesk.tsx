import type { DeskState } from './officeState';
import { displayedState } from './officeState';

interface PersonaDeskProps {
  desk: DeskState;
  x: number;
  y: number;
  onOpen: (id: string) => void;
  onHover: (id: string | null) => void;
}

function avatarText(desk: DeskState): string {
  if (desk.avatar.trim()) return desk.avatar.trim().slice(0, 2);
  return (desk.name.trim() || desk.id).slice(0, 1).toUpperCase();
}

export default function PersonaDesk({ desk, x, y, onOpen, onHover }: PersonaDeskProps) {
  const state = displayedState(desk);
  const working = state === 'working';
  const idle = state === 'idle';
  // consulting/in_team desks are dimmed; their motion is drawn by the overlay layer.
  const dimmed = state === 'consulting' || state === 'in_team';

  return (
    <div
      className={`persona-desk ${dimmed ? 'is-dimmed' : ''}`}
      style={{ left: x, top: y }}
      role="button"
      tabIndex={0}
      title={`${desk.name || desk.id} · ${state}`}
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
      <svg viewBox="0 0 132 96" width="132" height="96" aria-hidden="true">
        <rect x="2" y="44" width="128" height="44" rx="7" fill="#c79a5b" />
        <rect
          className={working ? 'screen-lit' : 'screen-game'}
          x="36" y="6" width="56" height="38" rx="4"
        />
        {idle && (
          <>
            <rect className="game-block" x="46" y="16" width="9" height="9" rx="2" fill="#ffd166" />
            <rect className="game-block b2" x="64" y="26" width="9" height="9" rx="2" fill="#06d6a0" />
          </>
        )}
        <rect x="60" y="44" width="11" height="8" fill="#3a3f4b" />
        <circle cx="66" cy="80" r="16" fill="#4f86c6" />
        <circle cx="66" cy="58" r="11" fill="#f2c79a" />
        <text x="66" y="62" textAnchor="middle" fontSize="11">{avatarText(desk)}</text>
        {working && (
          <g>
            <rect className="typing-hand" x="50" y="76" width="11" height="6" rx="3" fill="#f2c79a" />
            <rect className="typing-hand h2" x="71" y="76" width="11" height="6" rx="3" fill="#f2c79a" />
          </g>
        )}
      </svg>
      <div className="desk-name">{desk.name || desk.id}</div>
    </div>
  );
}
