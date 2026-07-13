import { useEffect, useMemo, useRef, useState } from 'react';
import './office.css';
import './office-marvis-v3.css';
import PersonaDesk from './PersonaDesk';
import { PersonaDeskMarvisV3 } from './PersonaDeskMarvisV3';
import { useActivityStream } from './useActivityStream';
import { assignSeats, CELL_H, CELL_W, type OfficeLayout, type Seat } from './officeLayout';
import { displayedState, type DeskState } from './officeState';
import { useI18n } from './i18n';
import { useDelegationAnims, type DelegationAnim } from './useDelegationAnims';

interface OfficeViewProps {
  onOpenPersona: (id: string) => void;
}

const COLS = 4;

// 根据 taskHint 推断状态
function inferStatus(taskHint: string): 'working' | 'busy' | 'planning' | 'away' | 'creative' | 'focused' {
  const hint = taskHint.toLowerCase();
  if (!hint || hint.includes('离线') || hint.includes('休息') || hint.includes('offline')) return 'away';
  if (hint.includes('忙碌') || hint.includes('busy') || hint.includes('负载')) return 'busy';
  if (hint.includes('规划') || hint.includes('planning') || hint.includes('设计')) return 'planning';
  if (hint.includes('创意') || hint.includes('creative') || hint.includes('艺术')) return 'creative';
  if (hint.includes('专注') || hint.includes('focused') || hint.includes('深度')) return 'focused';
  return 'working';
}

function deskCenter(seat: Seat): { x: number; y: number } {
  return { x: seat.x + 90, y: seat.y + 70 };
}

function DelegationLine({ from, to }: { from: Seat; to: Seat }) {
  const a = deskCenter(from);
  const b = deskCenter(to);
  const midX = (a.x + b.x) / 2;
  const midY = Math.min(a.y, b.y) - 30;
  return (
    <svg className="delegation-line-svg" style={{ position: 'absolute', left: 0, top: 0, width: '100%', height: '100%', pointerEvents: 'none' }}>
      <path
        d={`M${a.x},${a.y} Q${midX},${midY} ${b.x},${b.y}`}
        fill="none"
        stroke="var(--accent, #c98a2b)"
        strokeWidth="2"
        strokeDasharray="8 4"
        className="delegation-line-path"
        opacity="0.6"
      />
      <circle cx={b.x} cy={b.y} r="4" fill="var(--accent, #c98a2b)" opacity="0.8">
        <animate attributeName="r" values="4;6;4" dur="1.5s" repeatCount="indefinite" />
      </circle>
    </svg>
  );
}

function WalkingFigure({ fromSeat, toSeat, avatar, phase }: {
  fromSeat: Seat;
  toSeat: Seat;
  avatar: string;
  phase: 'going' | 'returning';
}) {
  const from = phase === 'going' ? fromSeat : toSeat;
  const to = phase === 'going' ? toSeat : fromSeat;
  const [arrived, setArrived] = useState(false);

  useEffect(() => {
    setArrived(false);
    const t = requestAnimationFrame(() => setArrived(true));
    return () => cancelAnimationFrame(t);
  }, [from.x, from.y, to.x, to.y, phase]);

  const x = arrived ? to.x + 50 : from.x + 50;
  const y = arrived ? to.y + 70 : from.y + 70;

  return (
    <div
      className="delegation-walker"
      style={{ transform: `translate(${x}px, ${y}px)` }}
    >
      <svg viewBox="0 0 40 48" width="36" height="44">
        <circle cx="14" cy="12" r="10" fill="#fef3e2" stroke="#c98a2b" strokeWidth="1" />
        <text x="14" y="16" textAnchor="middle" fontSize="10">{avatar || '🏃'}</text>
        <rect x="4" y="22" width="20" height="14" rx="6" fill="#c98a2b" fillOpacity="0.3" stroke="#c98a2b" strokeWidth="0.8" />
        <g className="walker-legs">
          <line x1="10" y1="36" x2="6" y2="46" stroke="#c98a2b" strokeWidth="1.5" strokeLinecap="round" />
          <line x1="18" y1="36" x2="22" y2="46" stroke="#c98a2b" strokeWidth="1.5" strokeLinecap="round" />
        </g>
      </svg>
    </div>
  );
}

function TalkBubble({ seat, side }: { seat: Seat; side: 'left' | 'right' }) {
  const x = side === 'right' ? seat.x + 150 : seat.x + 10;
  const y = seat.y + 20;
  return (
    <div className="delegation-talk-bubble" style={{ left: x, top: y }}>
      <span className="talk-dots">
        <span className="dot" />
        <span className="dot" />
        <span className="dot" />
      </span>
    </div>
  );
}

function DelegationOverlay({ anim, seats }: { anim: DelegationAnim; seats: Map<string, Seat> }) {
  const fromSeat = seats.get(anim.fromId);
  const toSeat = seats.get(anim.toId);
  if (!fromSeat || !toSeat) return null;

  const fromDesk = { ...fromSeat };
  const toDesk = { ...toSeat };

  switch (anim.phase) {
    case 'walk_to':
      return <WalkingFigure fromSeat={fromDesk} toSeat={toDesk} avatar="" phase="going" />;

    case 'talk_assign':
      return (
        <>
          <TalkBubble seat={toDesk} side="right" />
          <TalkBubble seat={fromDesk} side="left" />
        </>
      );

    case 'walk_back':
      return (
        <>
          <WalkingFigure fromSeat={toDesk} toSeat={fromDesk} avatar="" phase="going" />
          <DelegationLine from={fromSeat} to={toSeat} />
        </>
      );

    case 'connected':
      return <DelegationLine from={fromSeat} to={toSeat} />;

    case 'report_walk':
      return (
        <>
          <DelegationLine from={fromSeat} to={toSeat} />
          <WalkingFigure fromSeat={toDesk} toSeat={fromDesk} avatar="" phase="going" />
        </>
      );

    case 'report_talk':
      return (
        <>
          <TalkBubble seat={fromDesk} side="right" />
          <TalkBubble seat={toDesk} side="left" />
        </>
      );

    case 'return_walk':
      return <WalkingFigure fromSeat={fromDesk} toSeat={toDesk} avatar="" phase="going" />;

    default:
      return null;
  }
}

export default function OfficeView({ onOpenPersona }: OfficeViewProps) {
  const { t } = useI18n();
  const { office, connected } = useActivityStream(true);
  const [hoverId, setHoverId] = useState<string | null>(null);
  const [useMarvisStyle, setUseMarvisStyle] = useState(true); // Toggle Marvis style
  const { anims, startDelegation, endDelegation } = useDelegationAnims();

  const ids = useMemo(() => Array.from(office.keys()).sort(), [office]);
  const idKey = ids.join(',');
  const [layoutState, setLayoutState] = useState<{ idKey: string; layout: OfficeLayout }>(() => ({
    idKey,
    layout: assignSeats(ids, undefined, COLS),
  }));
  let layout = layoutState.layout;
  if (layoutState.idKey !== idKey) {
    layout = assignSeats(ids, layoutState.layout.seats, COLS);
    setLayoutState({ idKey, layout });
  }

  // Track consult state changes to trigger delegation animations.
  const prevConsultsRef = useRef<Map<string, string>>(new Map());
  useEffect(() => {
    const current = new Map<string, string>();
    for (const d of office.values()) {
      if (d.consultingTo && d.consultingTo !== '?') {
        current.set(d.id, d.consultingTo);
      }
    }
    const prev = prevConsultsRef.current;

    // New consultations -> start delegation animation
    for (const [fromId, toId] of current) {
      if (!prev.has(fromId)) {
        startDelegation(fromId, toId);
      }
    }

    // Ended consultations -> trigger end animation
    for (const [fromId, toId] of prev) {
      if (!current.has(fromId)) {
        endDelegation(fromId, toId);
      }
    }

    prevConsultsRef.current = current;
  }, [office, startDelegation, endDelegation]);

  const desks = Array.from(office.values());
  const width = COLS * CELL_W + 64;
  const height = layout.rows * CELL_H + 80;

  // team clusters: group desks by teamId
  const teams = new Map<string, DeskState[]>();
  for (const d of desks) {
    if (d.teamId && d.teamId !== '?') {
      const arr = teams.get(d.teamId) ?? [];
      arr.push(d);
      teams.set(d.teamId, arr);
    }
  }

  const hovered = hoverId ? office.get(hoverId) : null;
  const hoveredSeat = hoverId ? layout.seats.get(hoverId) : null;

  return (
    <div className="office-view">
      <div className="office-hint">
        {connected ? t('office.live') : t('office.offline')} · {t('office.clickHint')}
        {' · '}
        <button 
          onClick={() => setUseMarvisStyle(!useMarvisStyle)}
          style={{ 
            background: 'none', 
            border: '1px solid var(--accent, #c98a2b)', 
            borderRadius: '4px',
            padding: '2px 8px',
            cursor: 'pointer',
            fontSize: '11px',
            color: 'var(--accent, #c98a2b)'
          }}
        >
          {useMarvisStyle ? '经典视图' : 'Marvis 视图'}
        </button>
      </div>
      <div className="office-floor" style={{ width, height }}>
        {/* team rings (behind desks) */}
        {Array.from(teams.entries()).map(([teamId, members]) => {
          const seats = members.map((m) => layout.seats.get(m.id)).filter(Boolean) as Array<{ x: number; y: number }>;
          if (seats.length === 0) return null;
          const minX = Math.min(...seats.map((s) => s.x)) - 10;
          const minY = Math.min(...seats.map((s) => s.y)) - 22;
          const maxX = Math.max(...seats.map((s) => s.x)) + CELL_W - 8;
          const maxY = Math.max(...seats.map((s) => s.y)) + CELL_H - 24;
          return (
            <div key={teamId}>
              <div className="office-team-ring" style={{ left: minX, top: minY, width: maxX - minX, height: maxY - minY }} />
              <div className="office-team-label" style={{ left: minX + 8, top: minY + 2 }}>👥 {t('office.team')}</div>
            </div>
          );
        })}

        {/* delegation connection lines and animations */}
        {Array.from(anims.values()).map((anim) => (
          <DelegationOverlay key={anim.key} anim={anim} seats={layout.seats} />
        ))}

        {/* desks */}
        {desks.map((d) => {
          const seat = layout.seats.get(d.id);
          if (!seat) return null;
          return useMarvisStyle ? (
            <div key={d.id} style={{ position: 'absolute', left: seat.x, top: seat.y }}>
              <PersonaDeskMarvisV3
                name={d.name}
                role={d.name} // 使用 name 作为 role，或后续扩展 DeskState
                status={inferStatus(d.taskHint || '')}
                taskHint={d.taskHint}
                onClick={() => onOpenPersona(d.id)}
              />
            </div>
          ) : (
            <PersonaDesk key={d.id} desk={d} x={seat.x} y={seat.y} onOpen={onOpenPersona} onHover={setHoverId} />
          );
        })}

        {/* hover detail card */}
        {hovered && hoveredSeat && (
          <div className="office-card" style={{ left: hoveredSeat.x + 190, top: hoveredSeat.y }}>
            <strong>{hovered.avatar} {hovered.name || hovered.id}</strong>
            <span className="muted">{t(`office.state.${displayedState(hovered)}`)}</span>
            {hovered.taskHint && <div className="muted">{hovered.taskHint}</div>}
            {hovered.delegatedFrom && <div className="muted">{t('office.delegatedBy')} {hovered.delegatedFrom}</div>}
            {hovered.model && <div className="muted">{hovered.model}</div>}
          </div>
        )}

        {desks.length === 0 && <div className="office-hint">{t('office.empty')}</div>}
      </div>
    </div>
  );
}
