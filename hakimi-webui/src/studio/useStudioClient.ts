import { useCallback, useEffect, useRef, useState } from 'react';
import {
  type AttachRole,
  type CheckpointView,
  type DeviceSummary,
  type SessionSummary,
  type StudioCommand,
  type StudioEvent,
  type StudioEventEnvelope,
  type WorkspaceEntry,
  reqId,
  studioWsUrl,
} from './studioProtocol';

export type StudioChatMessage = {
  id: string;
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: string;
  runId?: string;
};

export type StudioConnectionState = 'connecting' | 'open' | 'closed' | 'error';

export type StudioRole = AttachRole;

export function useStudioClient() {
  const wsRef = useRef<WebSocket | null>(null);
  const lastSeqRef = useRef(0);
  const filePathRef = useRef<string | null>(null);
  const dirPathRef = useRef('');
  const sessionIdRef = useRef<string | null>(null);
  const [conn, setConn] = useState<StudioConnectionState>('connecting');
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [messages, setMessages] = useState<StudioChatMessage[]>([]);
  const [streaming, setStreaming] = useState('');
  const [busy, setBusy] = useState(false);
  const [entries, setEntries] = useState<WorkspaceEntry[]>([]);
  const [dirPath, setDirPath] = useState('');
  const [filePath, setFilePath] = useState<string | null>(null);
  const [fileContent, setFileContent] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [devices, setDevices] = useState<DeviceSummary[]>([]);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [checkpoints, setCheckpoints] = useState<CheckpointView[]>([]);
  const [role, setRole] = useState<StudioRole>('controller');
  const [activeRunner, setActiveRunner] = useState<string | null>(null);
  const [deviceId] = useState(() => {
    const key = 'hakimi-studio-device-id';
    const existing = localStorage.getItem(key);
    if (existing) return existing;
    const id = `web-${reqId('dev')}`;
    localStorage.setItem(key, id);
    return id;
  });

  const pendingList = useRef<((e: WorkspaceEntry[]) => void) | null>(null);
  const pendingRead = useRef<((content: string) => void) | null>(null);

  // Keep refs in sync for event handlers without rebinding.
  useEffect(() => {
    filePathRef.current = filePath;
  }, [filePath]);
  useEffect(() => {
    dirPathRef.current = dirPath;
  }, [dirPath]);
  useEffect(() => {
    sessionIdRef.current = sessionId;
  }, [sessionId]);

  const send = useCallback((cmd: StudioCommand) => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) {
      setError('Studio WebSocket not connected');
      return;
    }
    ws.send(JSON.stringify(cmd));
  }, []);

  const handleEvent = useCallback(
    (env: StudioEventEnvelope) => {
      if (typeof env.seq === 'number' && env.seq > lastSeqRef.current) {
        lastSeqRef.current = env.seq;
      }
      const ev = env.event as StudioEvent;
      const t = ev.type;

      if (t === 'hello_ok') {
        setConn('open');
        setError(null);
        return;
      }
      if (t === 'hello_error') {
        setError(String((ev as { message: string }).message));
        setConn('error');
        return;
      }
      if (t === 'session_created') {
        const e = ev as {
          session_id: string;
          active_runner_device_id: string;
        };
        setSessionId(e.session_id);
        setActiveRunner(e.active_runner_device_id);
        setRole('controller');
        setMessages([]);
        setStreaming('');
        lastSeqRef.current = env.seq ?? lastSeqRef.current;
        return;
      }
      if (t === 'session_snapshot') {
        const e = ev as {
          session_id: string;
          last_seq: number;
          active_runner_device_id: string;
          messages: Array<{ role: string; content: string }>;
        };
        setSessionId(e.session_id);
        setActiveRunner(e.active_runner_device_id);
        setMessages(
          (e.messages ?? []).map((m, i) => ({
            id: `snap-${i}`,
            role: (m.role as StudioChatMessage['role']) || 'assistant',
            content: m.content,
          })),
        );
        lastSeqRef.current = e.last_seq ?? lastSeqRef.current;
        return;
      }
      if (t === 'session_listed') {
        setSessions((ev as { sessions: SessionSummary[] }).sessions ?? []);
        return;
      }
      if (t === 'session_reset') {
        setError('Event window gap — re-attaching session…');
        const sid = (ev as { session_id: string }).session_id;
        send({
          type: 'session_attach',
          session_id: sid,
          role,
        });
        return;
      }
      if (t === 'run_started') {
        setBusy(true);
        setStreaming('');
        return;
      }
      if (t === 'run_queued') {
        setMessages((m) => [
          ...m,
          {
            id: reqId('sys'),
            role: 'system',
            content: `Queued at position ${(ev as { position: number }).position}`,
          },
        ]);
        return;
      }
      if (t === 'message_delta') {
        setStreaming((s) => s + (ev as { delta: string }).delta);
        return;
      }
      if (t === 'message_completed') {
        const text = (ev as { text: string }).text;
        const runId = (ev as { run_id: string }).run_id;
        setMessages((m) => [
          ...m,
          { id: reqId('a'), role: 'assistant', content: text, runId },
        ]);
        setStreaming('');
        return;
      }
      if (t === 'tool_started') {
        const name = (ev as { name: string }).name;
        setMessages((m) => [
          ...m,
          { id: reqId('t'), role: 'tool', content: `⚙ ${name}` },
        ]);
        return;
      }
      if (t === 'session_ended') {
        setBusy(false);
        return;
      }
      if (t === 'runner_changed') {
        const e = ev as {
          active_runner_device_id: string;
          from_device_id?: string | null;
        };
        setActiveRunner(e.active_runner_device_id);
        setMessages((m) => [
          ...m,
          {
            id: reqId('sys'),
            role: 'system',
            content: `Runner → ${e.active_runner_device_id}${
              e.from_device_id ? ` (from ${e.from_device_id})` : ''
            }`,
          },
        ]);
        return;
      }
      if (t === 'device_registered' || t === 'devices_listed') {
        if (t === 'devices_listed') {
          setDevices((ev as { devices: DeviceSummary[] }).devices ?? []);
        } else {
          send({ type: 'devices_list' });
        }
        return;
      }
      if (t === 'workspace_listed') {
        const listed = ev as { path: string; entries: WorkspaceEntry[] };
        setDirPath(listed.path ?? '');
        setEntries(listed.entries ?? []);
        pendingList.current?.(listed.entries ?? []);
        pendingList.current = null;
        return;
      }
      if (t === 'workspace_content') {
        const c = ev as { path: string; content: string };
        setFilePath(c.path);
        setFileContent(c.content);
        pendingRead.current?.(c.content);
        pendingRead.current = null;
        return;
      }
      if (t === 'checkpoint_created' || t === 'checkpoint_restored') {
        const cp = (ev as { checkpoint: CheckpointView }).checkpoint;
        setCheckpoints((list) => {
          const rest = list.filter((c) => c.id !== cp.id);
          return [cp, ...rest];
        });
        if (t === 'checkpoint_restored') {
          setMessages((m) => [
            ...m,
            {
              id: reqId('sys'),
              role: 'system',
              content: `Restored checkpoint ${cp.id}${cp.label ? ` (${cp.label})` : ''}`,
            },
          ]);
          const fp = filePathRef.current;
          const dp = dirPathRef.current;
          const sid = sessionIdRef.current ?? undefined;
          if (fp) {
            send({ type: 'workspace_read', path: fp, session_id: sid });
          }
          send({ type: 'workspace_list', path: dp, session_id: sid });
        } else {
          setMessages((m) => [
            ...m,
            {
              id: reqId('sys'),
              role: 'system',
              content: `Checkpoint ${cp.id}${cp.label ? ` (${cp.label})` : ''} · ${cp.files.length} file(s)`,
            },
          ]);
        }
        return;
      }
      if (t === 'checkpoints_listed') {
        setCheckpoints((ev as { checkpoints: CheckpointView[] }).checkpoints ?? []);
        return;
      }
      if (t === 'error') {
        const code = (ev as { code?: string | null }).code;
        const msg = String((ev as { message: string }).message);
        if (code === 'viewer_readonly') {
          setError(`Viewer only — ${msg}`);
          setRole('viewer');
        } else {
          setError(msg);
        }
        setBusy(false);
        return;
      }
    },
    [role, send],
  );

  useEffect(() => {
    let closed = false;
    let retry = 0;
    let timer: number | undefined;

    const connect = () => {
      if (closed) return;
      setConn('connecting');
      const ws = new WebSocket(studioWsUrl());
      wsRef.current = ws;

      ws.onopen = () => {
        retry = 0;
        ws.send(
          JSON.stringify({
            type: 'hello',
            device_id: deviceId,
            kind: 'web',
            device_name: 'Hakimi Studio Web',
            protocol_version: 1,
          } satisfies StudioCommand),
        );
        const params = new URLSearchParams(window.location.search);
        const attachSid = params.get('session');
        const attachRole = (params.get('role') as AttachRole) || 'controller';
        if (attachSid) {
          setRole(attachRole);
          ws.send(
            JSON.stringify({
              type: 'session_attach',
              session_id: attachSid,
              after_seq: lastSeqRef.current || undefined,
              role: attachRole,
            } satisfies StudioCommand),
          );
        } else {
          ws.send(
            JSON.stringify({
              type: 'session_create',
              title: 'Studio',
              prefer_runner: 'local',
            } satisfies StudioCommand),
          );
        }
        ws.send(
          JSON.stringify({
            type: 'workspace_list',
            path: '',
          } satisfies StudioCommand),
        );
        ws.send(JSON.stringify({ type: 'devices_list' } satisfies StudioCommand));
        ws.send(JSON.stringify({ type: 'session_list', limit: 20 } satisfies StudioCommand));
        ws.send(JSON.stringify({ type: 'checkpoint_list' } satisfies StudioCommand));
      };

      ws.onmessage = (msg) => {
        try {
          const env = JSON.parse(String(msg.data)) as StudioEventEnvelope;
          handleEvent(env);
        } catch {
          /* ignore malformed */
        }
      };

      ws.onerror = () => {
        setConn('error');
      };

      ws.onclose = () => {
        setConn('closed');
        wsRef.current = null;
        if (!closed) {
          const delay = Math.min(8000, 500 * 2 ** retry);
          retry += 1;
          timer = window.setTimeout(connect, delay);
        }
      };
    };

    connect();
    return () => {
      closed = true;
      if (timer) window.clearTimeout(timer);
      wsRef.current?.close();
    };
  }, [deviceId, handleEvent]);

  const listDir = useCallback(
    (path: string) => {
      send({ type: 'workspace_list', path, session_id: sessionId ?? undefined });
    },
    [send, sessionId],
  );

  const readFile = useCallback(
    (path: string) => {
      send({ type: 'workspace_read', path, session_id: sessionId ?? undefined });
    },
    [send, sessionId],
  );

  const submit = useCallback(
    (text: string, preempt = false) => {
      if (!sessionId || !text.trim()) return;
      if (role === 'viewer') {
        setError('Viewer mode — switch to controller or request handoff');
        return;
      }
      const client_request_id = reqId('chat');
      setMessages((m) => [
        ...m,
        { id: client_request_id, role: 'user', content: text.trim() },
      ]);
      send({
        type: 'chat_submit',
        session_id: sessionId,
        text: text.trim(),
        client_request_id,
        preempt,
      });
    },
    [send, sessionId, role],
  );

  const cancel = useCallback(() => {
    if (!sessionId) return;
    send({ type: 'chat_cancel', session_id: sessionId });
  }, [send, sessionId]);

  const refreshDevices = useCallback(() => {
    send({ type: 'devices_list' });
  }, [send]);

  const refreshSessions = useCallback(() => {
    send({ type: 'session_list', limit: 30 });
  }, [send]);

  const refreshCheckpoints = useCallback(() => {
    send({ type: 'checkpoint_list', session_id: sessionId ?? undefined });
  }, [send, sessionId]);

  const createCheckpoint = useCallback(
    (label?: string, paths?: string[]) => {
      if (role === 'viewer') {
        setError('Viewer mode — cannot create checkpoint');
        return;
      }
      send({
        type: 'checkpoint_create',
        session_id: sessionId ?? undefined,
        label,
        paths: paths ?? (filePath ? [filePath] : []),
      });
    },
    [send, sessionId, role, filePath],
  );

  const restoreCheckpoint = useCallback(
    (checkpointId: string) => {
      if (role === 'viewer') {
        setError('Viewer mode — cannot restore checkpoint');
        return;
      }
      send({
        type: 'checkpoint_restore',
        session_id: sessionId ?? undefined,
        checkpoint_id: checkpointId,
      });
    },
    [send, sessionId, role],
  );

  const attachSession = useCallback(
    (sid: string, asRole: AttachRole = 'viewer') => {
      setRole(asRole);
      lastSeqRef.current = 0;
      setMessages([]);
      setStreaming('');
      send({
        type: 'session_attach',
        session_id: sid,
        role: asRole,
      });
      const url = new URL(window.location.href);
      url.searchParams.set('session', sid);
      url.searchParams.set('role', asRole);
      window.history.replaceState({}, '', url.toString());
    },
    [send],
  );

  const handoffTo = useCallback(
    (toDeviceId: string) => {
      if (!sessionId) return;
      send({
        type: 'runner_handoff',
        session_id: sessionId,
        to_device_id: toDeviceId,
        from_device_id: deviceId,
      });
    },
    [send, sessionId, deviceId],
  );

  const becomeController = useCallback(() => {
    if (!sessionId) return;
    setRole('controller');
    send({
      type: 'session_attach',
      session_id: sessionId,
      after_seq: lastSeqRef.current || undefined,
      role: 'controller',
    });
  }, [send, sessionId]);

  const becomeViewer = useCallback(() => {
    if (!sessionId) return;
    setRole('viewer');
    send({
      type: 'session_attach',
      session_id: sessionId,
      after_seq: lastSeqRef.current || undefined,
      role: 'viewer',
    });
  }, [send, sessionId]);

  return {
    conn,
    sessionId,
    messages,
    streaming,
    busy,
    entries,
    dirPath,
    filePath,
    fileContent,
    error,
    setError,
    devices,
    sessions,
    checkpoints,
    role,
    activeRunner,
    deviceId,
    listDir,
    readFile,
    submit,
    cancel,
    refreshDevices,
    refreshSessions,
    refreshCheckpoints,
    createCheckpoint,
    restoreCheckpoint,
    attachSession,
    handoffTo,
    becomeController,
    becomeViewer,
  };
}
