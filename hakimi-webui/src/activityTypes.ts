export type PersonaState = 'idle' | 'working' | 'consulting' | 'in_team';

export interface PersonaActivity {
  id: string;
  name: string;
  avatar: string;
  state: PersonaState;
  task_hint?: string;
  model?: string;
  team_id?: string;
}

export interface ActivitySnapshotResponse {
  personas: PersonaActivity[];
}

// Discriminated union matching #[serde(tag = "type", rename_all = "snake_case")].
export type ActivityEvent =
  | { type: 'persona_created'; id: string; name: string; avatar: string }
  | { type: 'persona_updated'; id: string; name: string; avatar: string }
  | { type: 'persona_deleted'; id: string }
  | { type: 'turn_started'; persona_id: string; task_hint?: string | null; model?: string | null }
  | { type: 'turn_ended'; persona_id: string }
  | { type: 'consult_started'; from_id: string; to_id: string; task_hint?: string | null }
  | { type: 'consult_ended'; from_id: string; to_id: string }
  | { type: 'team_formed'; team_id: string; lead_id: string; member_ids: string[]; task_hint?: string | null }
  | { type: 'team_disbanded'; team_id: string };
