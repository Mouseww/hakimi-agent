import { describe, expect, it } from 'vitest';
import { displayedState, reduceActivity, seedOffice, type DeskState } from './officeState';

function desk(state: Partial<DeskState>): DeskState {
  return { id: 'x', name: 'X', avatar: '🙂', working: false, ...state };
}

describe('officeState', () => {
  it('seeds desks from a snapshot', () => {
    const s = seedOffice([{ id: 'a', name: 'A', avatar: '🤖', state: 'working' }]);
    expect(s.get('a')!.working).toBe(true);
    expect(displayedState(s.get('a')!)).toBe('working');
  });

  it('turn start/end toggles working', () => {
    let s = new Map<string, DeskState>([['a', desk({ id: 'a' })]]);
    s = reduceActivity(s, { type: 'turn_started', persona_id: 'a', task_hint: 'fix', model: 'opus' });
    expect(displayedState(s.get('a')!)).toBe('working');
    expect(s.get('a')!.taskHint).toBe('fix');
    s = reduceActivity(s, { type: 'turn_ended', persona_id: 'a' });
    expect(displayedState(s.get('a')!)).toBe('idle');
  });

  it('consult overlays working and restores it on end', () => {
    let s = new Map<string, DeskState>([['a', desk({ id: 'a', working: true })]]);
    s = reduceActivity(s, { type: 'consult_started', from_id: 'a', to_id: 'b', task_hint: null });
    expect(displayedState(s.get('a')!)).toBe('consulting');
    expect(s.get('a')!.consultingTo).toBe('b');
    s = reduceActivity(s, { type: 'consult_ended', from_id: 'a', to_id: 'b' });
    expect(displayedState(s.get('a')!)).toBe('working'); // base preserved
  });

  it('team masks other states until disbanded', () => {
    let s = new Map<string, DeskState>([
      ['a', desk({ id: 'a', working: true })],
      ['b', desk({ id: 'b' })],
    ]);
    s = reduceActivity(s, { type: 'team_formed', team_id: 't1', lead_id: 'a', member_ids: ['b'], task_hint: null });
    expect(displayedState(s.get('a')!)).toBe('in_team');
    expect(displayedState(s.get('b')!)).toBe('in_team');
    s = reduceActivity(s, { type: 'team_disbanded', team_id: 't1' });
    expect(displayedState(s.get('a')!)).toBe('working');
    expect(displayedState(s.get('b')!)).toBe('idle');
  });

  it('persona_created adds a desk, persona_deleted removes it', () => {
    let s = new Map<string, DeskState>();
    s = reduceActivity(s, { type: 'persona_created', id: 'a', name: 'A', avatar: '🤖' });
    expect(s.has('a')).toBe(true);
    s = reduceActivity(s, { type: 'persona_deleted', id: 'a' });
    expect(s.has('a')).toBe(false);
  });
});
