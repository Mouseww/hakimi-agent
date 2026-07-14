import type { ActivityEvent, PersonaActivity, PersonaState } from './activityTypes';

export interface DeskState {
  id: string;
  name: string;
  avatar: string;
  working: boolean;            // base
  consultingTo?: string;       // overlay: who this agent is consulting
  delegatedFrom?: string;      // overlay: who delegated work to this agent
  teamId?: string;             // overlay
  taskHint?: string;
  model?: string;
}

export type OfficeState = Map<string, DeskState>;

/** Displayed state from base + overlay (priority: in_team > consulting > working/delegated > idle). */
export function displayedState(desk: DeskState): PersonaState {
  if (desk.teamId) return 'in_team';
  if (desk.consultingTo) return 'consulting';
  if (desk.working || desk.delegatedFrom) return 'working';
  return 'idle';
}

/** Build initial office state from the snapshot rows (displayed-state approximation). */
export function seedOffice(rows: PersonaActivity[]): OfficeState {
  const map: OfficeState = new Map();
  for (const r of rows) {
    map.set(r.id, {
      id: r.id,
      name: r.name,
      avatar: r.avatar,
      working: r.state === 'working' && !r.delegated_from,
      consultingTo: r.consulting_to ?? (r.state === 'consulting' ? '?' : undefined),
      delegatedFrom: r.delegated_from,
      teamId: r.team_id ?? (r.state === 'in_team' ? '?' : undefined),
      taskHint: r.task_hint,
      model: r.model,
    });
  }
  return map;
}

/** Apply one event, returning a NEW map (immutable update). */
export function reduceActivity(state: OfficeState, event: ActivityEvent): OfficeState {
  const map: OfficeState = new Map(state);
  const set = (id: string, patch: Partial<DeskState>) => {
    const base = map.get(id) ?? { id, name: id, avatar: '', working: false };
    map.set(id, { ...base, ...patch });
  };

  switch (event.type) {
    case 'persona_created':
    case 'persona_updated': {
      const prev = map.get(event.id);
      set(event.id, { name: event.name, avatar: event.avatar, working: prev?.working ?? false });
      break;
    }
    case 'persona_deleted':
      map.delete(event.id);
      break;
    case 'turn_started':
      set(event.persona_id, {
        working: true,
        taskHint: event.task_hint ?? undefined,
        model: event.model ?? undefined,
      });
      break;
    case 'turn_ended':
      set(event.persona_id, { working: false, taskHint: undefined, model: undefined });
      break;
    case 'consult_started':
      set(event.from_id, { consultingTo: event.to_id });
      set(event.to_id, {
        delegatedFrom: event.from_id,
        taskHint: event.task_hint ?? undefined,
      });
      break;
    case 'consult_ended':
      // 清除委派状态，并恢复空闲状态
      set(event.from_id, { 
        consultingTo: undefined,
        working: false,         // 委派者恢复休息
        taskHint: undefined,
      });
      set(event.to_id, { 
        delegatedFrom: undefined,
        working: false,         // 被委派者完成任务，恢复休息
        taskHint: undefined,
      });
      break;
    case 'team_formed':
      for (const id of [event.lead_id, ...event.member_ids]) {
        set(id, { teamId: event.team_id });
      }
      break;
    case 'team_disbanded':
      // Snapshot entries before writing so we never mutate the map mid-iteration.
      for (const [id, d] of Array.from(map)) {
        if (d.teamId === event.team_id) {
          map.set(id, { ...d, teamId: undefined });
        }
      }
      break;
  }
  return map;
}
