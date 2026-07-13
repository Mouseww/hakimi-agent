import { useEffect, useMemo, useRef, useState } from 'react';
import './office.css';
import './office-marvis-v3.css';
import PersonaDesk from './PersonaDesk';
import { PersonaDeskMarvisV3 } from './PersonaDeskMarvisV3';
import { useActivityStream } from './useActivityStream';
import { assignSeats, CELL_H, CELL_W, type OfficeLayout, type Seat } from './officeLayout';
import { displayedState, type DeskState } from './officeState';
import { useI18n } from './i18n';
import { useDelegationAnims, type DelegationAnim, type DelegationPhase } from './useDelegationAnims';
import { WalkingCat } from './WalkingCat';
import { AgentProgressModal } from './AgentProgressModal';

interface OfficeViewProps {
  onOpenPersona: (id: string) => void;
}

const COLS = 4;

// 根据 DeskState 推断状态
function inferStatus(
  desk: { id: string; taskHint?: string; consultingTo?: string; delegatedFrom?: string },
  anims: Map<string, DelegationAnim>
): 'working' | 'busy' | 'planning' | 'away' | 'creative' | 'focused' {
  // 检查是否正在委派任务（consultingTo）或接受委派（delegatedFrom）
  // 但排除"正在回家路上"的阶段（委派已经结束，只是动画尚未完成）
  const endingPhases: DelegationPhase[] = ['report_walk', 'report_talk', 'return_walk'];
  const isEnding = Array.from(anims.values()).some(
    anim => (anim.fromId === desk.id || anim.toId === desk.id) && endingPhases.includes(anim.phase)
  );
  
  if (!isEnding && (desk.consultingTo || desk.delegatedFrom)) {
    return 'busy';
  }
  
  const hint = (desk.taskHint || '').toLowerCase();
  if (!hint || hint.includes('离线') || hint.includes('休息') || hint.includes('offline')) return 'away';
  if (hint.includes('忙碌') || hint.includes('busy') || hint.includes('负载')) return 'busy';
  if (hint.includes('规划') || hint.includes('planning') || hint.includes('设计')) return 'planning';
  if (hint.includes('创意') || hint.includes('creative') || hint.includes('艺术')) return 'creative';
  if (hint.includes('专注') || hint.includes('focused') || hint.includes('深度')) return 'focused';
  return 'working';
}

// 为每个 agent 生成唯一的颜色（基于 ID 哈希）
function generateAgentColors(id: string): { scarf: string; tail: string } {
  let hash = 0;
  for (let i = 0; i < id.length; i++) {
    hash = ((hash << 5) - hash) + id.charCodeAt(i);
    hash |= 0;
  }
  
  const hue = Math.abs(hash) % 360;
  const scarf = `hsl(${hue}, 70%, 55%)`;
  const tail = `hsl(${hue}, 65%, 68%)`;
  
  return { scarf, tail };
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

function DelegationOverlay({ 
  anim, 
  seats 
}: { 
  anim: DelegationAnim; 
  seats: Map<string, Seat>;
}) {
  const fromSeat = seats.get(anim.fromId);
  const toSeat = seats.get(anim.toId);
  if (!fromSeat || !toSeat) return null;

  const fromDesk = { ...fromSeat };
  const toDesk = { ...toSeat };
  
  // 获取委派方的颜色
  const fromColors = generateAgentColors(anim.fromId);
  const toColors = generateAgentColors(anim.toId);

  switch (anim.phase) {
    case 'walk_to':
      return (
        <WalkingCat 
          fromSeat={fromDesk} 
          toSeat={toDesk} 
          phase="going" 
          scarfColor={fromColors.scarf}
          tailColor={fromColors.tail}
        />
      );

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
          <WalkingCat 
            fromSeat={toDesk} 
            toSeat={fromDesk} 
            phase="going" 
            scarfColor={fromColors.scarf}
            tailColor={fromColors.tail}
          />
          <DelegationLine from={fromSeat} to={toSeat} />
        </>
      );

    case 'connected':
      return <DelegationLine from={fromSeat} to={toSeat} />;

    case 'report_walk':
      return (
        <>
          <DelegationLine from={fromSeat} to={toSeat} />
          <WalkingCat 
            fromSeat={toDesk} 
            toSeat={fromDesk} 
            phase="going" 
            scarfColor={toColors.scarf}
            tailColor={toColors.tail}
          />
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
      return (
        <WalkingCat 
          fromSeat={fromDesk} 
          toSeat={toDesk} 
          phase="going" 
          scarfColor={toColors.scarf}
          tailColor={toColors.tail}
        />
      );

    default:
      return null;
  }
}

export default function OfficeView({ onOpenPersona }: OfficeViewProps) {
  const { t } = useI18n();
  const { office, connected } = useActivityStream(true);
  const [hoverId, setHoverId] = useState<string | null>(null);
  const [useMarvisStyle, setUseMarvisStyle] = useState(true); // Toggle Marvis style
  const [modalAgentId, setModalAgentId] = useState<string | null>(null); // Agent progress modal
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
  
  // 计算哪些 Agent 正在行走（需要隐藏工位猫咪）
  const walkingAgents = useMemo(() => {
    const walking = new Set<string>();
    for (const anim of anims.values()) {
      const walkPhases: DelegationPhase[] = ['walk_to', 'walk_back', 'report_walk', 'return_walk'];
      if (walkPhases.includes(anim.phase)) {
        // walk_to 和 walk_back: 委派方在行走
        if (anim.phase === 'walk_to' || anim.phase === 'walk_back') {
          walking.add(anim.fromId);
        }
        // report_walk 和 return_walk: 被委派方在行走
        if (anim.phase === 'report_walk' || anim.phase === 'return_walk') {
          walking.add(anim.toId);
        }
      }
    }
    return walking;
  }, [anims]);

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
          const colors = generateAgentColors(d.id);
          const isWalking = walkingAgents.has(d.id);
          return useMarvisStyle ? (
            <div key={d.id} style={{ position: 'absolute', left: seat.x, top: seat.y }}>
              <PersonaDeskMarvisV3
                name={d.name}
                role={d.name}
                status={inferStatus(d, anims)}
                taskHint={d.taskHint}
                onClick={() => setModalAgentId(d.id)}
                scarfColor={colors.scarf}
                tailColor={colors.tail}
                hideCat={isWalking}
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

      {/* Agent progress modal */}
      {modalAgentId && (
        <AgentProgressModal
          agentId={modalAgentId}
          agentName={office.get(modalAgentId)?.name || modalAgentId}
          onClose={() => setModalAgentId(null)}
        />
      )}
    </div>
  );
}
