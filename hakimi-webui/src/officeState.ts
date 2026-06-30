import type { ActivityEvent, PersonaActivity, PersonaState } from './activityTypes';

export interface DeskState {
  id: string;
  name: string;
  avatar: string;
  working: boolean;            // base
  consultingTo?: string;       // overlay
  teamId?: string;             // overlay
  taskHint?: string;
  model?: string;
}

export type OfficeState = Map<string, DeskState>;

/** Displayed state from base + overlay (priority: in_team > consulting > working > idle). */
export function displayedState(desk: DeskState): PersonaState {
  if (desk.teamId) return 'in_team';
  if (desk.consultingTo) return 'consulting';
  if (desk.working) return 'working';
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
      working: r.state === 'working',
      consultingTo: r.state === 'consulting' ? '?' : undefined,
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
      break;
    case 'consult_ended':
      set(event.from_id, { consultingTo: undefined });
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
