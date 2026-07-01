import { useCallback, useEffect, useState } from 'react';

export type DelegationPhase =
  | 'walk_to'       // delegator walks to delegate's desk
  | 'talk_assign'   // conversation at delegate's desk
  | 'walk_back'     // delegator walks back to own desk
  | 'connected'     // steady: connection line visible, delegate working
  | 'report_walk'   // delegate walks to delegator's desk
  | 'report_talk'   // conversation at delegator's desk
  | 'return_walk';  // delegate walks back to own desk

export interface DelegationAnim {
  key: string;
  fromId: string;
  toId: string;
  phase: DelegationPhase;
  phaseStart: number;
}

const PHASE_DURATIONS: Record<DelegationPhase, number> = {
  walk_to: 1400,
  talk_assign: 1500,
  walk_back: 1400,
  connected: Infinity,
  report_walk: 1400,
  report_talk: 1500,
  return_walk: 1400,
};

const START_SEQUENCE: DelegationPhase[] = ['walk_to', 'talk_assign', 'walk_back', 'connected'];
const END_SEQUENCE: DelegationPhase[] = ['report_walk', 'report_talk', 'return_walk'];

function nextPhase(phase: DelegationPhase): DelegationPhase | null {
  const startIdx = START_SEQUENCE.indexOf(phase);
  if (startIdx >= 0 && startIdx < START_SEQUENCE.length - 1) return START_SEQUENCE[startIdx + 1];
  const endIdx = END_SEQUENCE.indexOf(phase);
  if (endIdx >= 0 && endIdx < END_SEQUENCE.length - 1) return END_SEQUENCE[endIdx + 1];
  return null;
}

function animKey(fromId: string, toId: string): string {
  return `${fromId}->${toId}`;
}

export function useDelegationAnims() {
  const [anims, setAnims] = useState<Map<string, DelegationAnim>>(new Map());

  const tick = useCallback(() => {
    setAnims((prev) => {
      const now = Date.now();
      let changed = false;
      const next = new Map(prev);

      for (const [key, anim] of next) {
        const duration = PHASE_DURATIONS[anim.phase];
        if (duration === Infinity) continue;
        if (now - anim.phaseStart >= duration) {
          const np = nextPhase(anim.phase);
          if (np) {
            next.set(key, { ...anim, phase: np, phaseStart: now });
            changed = true;
          } else {
            next.delete(key);
            changed = true;
          }
        }
      }
      return changed ? next : prev;
    });
  }, []);

  useEffect(() => {
    const id = setInterval(tick, 200);
    return () => clearInterval(id);
  }, [tick]);

  const startDelegation = useCallback((fromId: string, toId: string) => {
    const key = animKey(fromId, toId);
    setAnims((prev) => {
      const next = new Map(prev);
      next.set(key, { key, fromId, toId, phase: 'walk_to', phaseStart: Date.now() });
      return next;
    });
  }, []);

  const endDelegation = useCallback((fromId: string, toId: string) => {
    const key = animKey(fromId, toId);
    setAnims((prev) => {
      const next = new Map(prev);
      const existing = next.get(key);
      if (existing) {
        next.set(key, { ...existing, phase: 'report_walk', phaseStart: Date.now() });
      }
      return next;
    });
  }, []);

  return { anims, startDelegation, endDelegation };
}
