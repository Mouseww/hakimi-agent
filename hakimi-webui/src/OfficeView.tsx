import { useMemo, useState } from 'react';
import './office.css';
import PersonaDesk from './PersonaDesk';
import { useActivityStream } from './useActivityStream';
import { assignSeats, CELL_H, CELL_W, type OfficeLayout } from './officeLayout';
import { displayedState, type DeskState } from './officeState';
import { useI18n } from './i18n';

interface OfficeViewProps {
  onOpenPersona: (id: string) => void;
}

const COLS = 4;

export default function OfficeView({ onOpenPersona }: OfficeViewProps) {
  const { t } = useI18n();
  const { office, connected } = useActivityStream(true);
  const [hoverId, setHoverId] = useState<string | null>(null);

  // Stable seat assignment: recompute only when the sorted id list changes, carrying
  // forward the previous seat map so existing desks stay put and freed slots are reused.
  const ids = useMemo(() => Array.from(office.keys()).sort(), [office]);
  const idKey = ids.join(',');
  const [layoutState, setLayoutState] = useState<{ idKey: string; layout: OfficeLayout }>(() => ({
    idKey,
    layout: assignSeats(ids, undefined, COLS),
  }));
  // React's documented "adjust state during render" pattern: on a persona-set change,
  // recompute seats and store them; React re-renders immediately with the new state
  // (single commit, no setTimeout). `next` is also used for this pass.
  let layout = layoutState.layout;
  if (layoutState.idKey !== idKey) {
    layout = assignSeats(ids, layoutState.layout.seats, COLS);
    setLayoutState({ idKey, layout });
  }

  const desks = Array.from(office.values());
  const width = COLS * CELL_W + 64;
  const height = layout.rows * CELL_H + 80;

  // consult runners: from-seat -> to-seat
  const runners = desks
    .filter((d) => displayedState(d) === 'consulting' && d.consultingTo && d.consultingTo !== '?')
    .map((d) => {
      const from = layout.seats.get(d.id);
      const to = layout.seats.get(d.consultingTo!);
      if (!from || !to) return null;
      return { id: d.id, avatar: d.avatar, from, to };
    })
    .filter(Boolean) as Array<{ id: string; avatar: string; from: { x: number; y: number }; to: { x: number; y: number } }>;

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

        {/* desks */}
        {desks.map((d) => {
          const seat = layout.seats.get(d.id);
          if (!seat) return null;
          return (
            <PersonaDesk key={d.id} desk={d} x={seat.x} y={seat.y} onOpen={onOpenPersona} onHover={setHoverId} />
          );
        })}

        {/* consult runners (CSS transition animates the position change) */}
        {runners.map((r) => (
          <div
            key={`run-${r.id}`}
            className="office-runner"
            style={{ transform: `translate(${r.to.x + 50}px, ${r.to.y + 70}px)` }}
            data-from={`${r.from.x},${r.from.y}`}
          >
            <svg viewBox="0 0 40 48" width="40" height="48">
              <circle cx="14" cy="14" r="11" fill="#f2c79a" />
              <rect x="2" y="24" width="24" height="18" rx="7" fill="#7d5ba6" />
              <text x="14" y="18" textAnchor="middle" fontSize="11">{r.avatar || '🏃'}</text>
              <text x="30" y="14" fontSize="11">📋</text>
            </svg>
          </div>
        ))}

        {/* hover detail card */}
        {hovered && hoveredSeat && (
          <div className="office-card" style={{ left: hoveredSeat.x + 190, top: hoveredSeat.y }}>
            <strong>{hovered.avatar} {hovered.name || hovered.id}</strong>
            <span className="muted">{t(`office.state.${displayedState(hovered)}`)}</span>
            {hovered.taskHint && <div className="muted">{hovered.taskHint}</div>}
            {hovered.model && <div className="muted">{hovered.model}</div>}
          </div>
        )}

        {desks.length === 0 && <div className="office-hint">{t('office.empty')}</div>}
      </div>
    </div>
  );
}
