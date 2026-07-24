/** Studio Protocol v1 — client-side types (mirror of hakimi-studio-api). */

export type PreferRunner = 'local' | 'server';
export type DeviceKind = 'desktop' | 'web' | 'server' | 'cli';
export type AttachRole = 'controller' | 'viewer';

export type DeviceSummary = {
  device_id: string;
  device_name?: string | null;
  kind: DeviceKind;
  is_runner: boolean;
  connected_at: string;
};

export type SessionSummary = {
  session_id: string;
  title: string;
  updated_at: string;
  active_runner_device_id: string;
  last_seq: number;
};

export type StudioCommand =
  | {
      type: 'hello';
      device_id: string;
      device_name?: string;
      kind?: DeviceKind;
      protocol_version?: number;
      token?: string;
    }
  | {
      type: 'session_create';
      workspace_id?: string;
      title?: string;
      prefer_runner?: PreferRunner;
    }
  | { type: 'session_list'; limit?: number }
  | {
      type: 'session_attach';
      session_id: string;
      after_seq?: number;
      role?: AttachRole;
    }
  | {
      type: 'chat_submit';
      session_id: string;
      text: string;
      client_request_id: string;
      preempt?: boolean;
    }
  | { type: 'chat_cancel'; session_id: string; run_id?: string }
  | {
      type: 'chat_preempt';
      session_id: string;
      text: string;
      client_request_id: string;
    }
  | {
      type: 'runner_handoff';
      session_id: string;
      to_device_id: string;
      from_device_id?: string;
    }
  | { type: 'devices_list' }
  | { type: 'workspace_list'; session_id?: string; path?: string }
  | { type: 'workspace_read'; session_id?: string; path: string }
  | {
      type: 'workspace_write';
      session_id?: string;
      path: string;
      content: string;
    }
  | {
      type: 'workspace_create';
      session_id?: string;
      path: string;
      is_dir?: boolean;
    }
  | {
      type: 'workspace_delete';
      session_id?: string;
      path: string;
      recursive?: boolean;
    }
  | {
      type: 'workspace_grep';
      session_id?: string;
      path?: string;
      pattern: string;
      limit?: number;
    }
  | {
      type: 'checkpoint_create';
      session_id?: string;
      label?: string;
      paths?: string[];
    }
  | { type: 'checkpoint_list'; session_id?: string }
  | {
      type: 'checkpoint_restore';
      session_id?: string;
      checkpoint_id: string;
    }
  | { type: 'ping'; nonce?: string };

export type WorkspaceEntry = {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  git_status?: string | null;
};

export type CheckpointView = {
  id: string;
  label?: string | null;
  created_at: string;
  files: string[];
  path: string;
};

export type StudioEvent =
  | { type: 'hello_ok'; device_id: string; protocol_version: number; prefer_runner: PreferRunner }
  | { type: 'hello_error'; message: string }
  | {
      type: 'session_created';
      session_id: string;
      title: string;
      active_runner_device_id: string;
      prefer_runner: PreferRunner;
    }
  | {
      type: 'session_snapshot';
      session_id: string;
      last_seq: number;
      title: string;
      active_runner_device_id: string;
      messages: Array<{ role: string; content: string }>;
      queue_depth: number;
    }
  | {
      type: 'session_listed';
      sessions: SessionSummary[];
    }
  | {
      type: 'session_reset';
      session_id: string;
      reason: string;
      last_seq: number;
      window_oldest_seq?: number | null;
    }
  | { type: 'run_started'; session_id: string; run_id: string; client_request_id: string }
  | { type: 'run_queued'; session_id: string; client_request_id: string; position: number }
  | { type: 'run_preempted'; session_id: string; run_id: string; reason: string }
  | { type: 'message_delta'; session_id: string; run_id: string; delta: string }
  | { type: 'message_completed'; session_id: string; run_id: string; text: string }
  | { type: 'tool_started'; session_id: string; run_id: string; name: string; call_id: string }
  | { type: 'tool_completed'; session_id: string; run_id: string; call_id: string; ok: boolean }
  | { type: 'session_ended'; session_id: string; run_id: string; reason: string }
  | {
      type: 'runner_changed';
      session_id: string;
      active_runner_device_id: string;
      from_device_id?: string | null;
    }
  | {
      type: 'device_registered';
      device_id: string;
      device_name?: string | null;
      kind: DeviceKind;
      is_runner: boolean;
    }
  | { type: 'devices_listed'; devices: DeviceSummary[] }
  | {
      type: 'workspace_listed';
      session_id?: string | null;
      path: string;
      entries: WorkspaceEntry[];
    }
  | {
      type: 'workspace_content';
      session_id?: string | null;
      path: string;
      content: string;
    }
  | { type: 'workspace_written'; session_id?: string | null; path: string }
  | {
      type: 'checkpoint_created';
      session_id?: string | null;
      checkpoint: CheckpointView;
    }
  | {
      type: 'checkpoints_listed';
      session_id?: string | null;
      checkpoints: CheckpointView[];
    }
  | {
      type: 'checkpoint_restored';
      session_id?: string | null;
      checkpoint: CheckpointView;
    }
  | { type: 'error'; session_id?: string | null; message: string; code?: string | null }
  | { type: 'pong'; nonce?: string | null }
  | { type: string; [key: string]: unknown };

export type StudioEventEnvelope = {
  seq: number;
  session_id?: string | null;
  event: StudioEvent;
  ts?: string | null;
};

export function studioWsUrl(): string {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${proto}//${window.location.host}/v1/studio`;
}

export function reqId(prefix = 'req'): string {
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(16).slice(2, 8)}`;
}
