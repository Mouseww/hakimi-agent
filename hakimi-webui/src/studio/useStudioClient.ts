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
  /** Tool call id when role === 'tool' */
  callId?: string;
  toolName?: string;
  toolStatus?: 'running' | 'done' | 'error';
};

/** Normalize workspace-relative paths for crumbs / list state. */
export function normalizeStudioPath(path: string | null | undefined): string {
  if (!path) return '';
  return path
    .replace(/\\/g, '/')
    .replace(/^\.\/+/, '')
    .replace(/^\/+/, '')
    .replace(/\/+$/, '');
}

export type StudioConnectionState = 'connecting' | 'open' | 'closed' | 'error';

export type StudioRole = AttachRole;

export function useStudioClient() {
  const wsRef = useRef<WebSocket | null>(null);
  const lastSeqRef = useRef(0);
  const filePathRef = useRef<string | null>(null);
  const dirPathRef = useRef('');
  const sessionIdRef = useRef<string | null>(null);
  /** Live stream buffer (sync) so tool boundaries can flush without races. */
  const streamingRef = useRef('');
  /** True if any message_delta arrived for the active run. */
  const streamedThisRunRef = useRef(false);
  const activeRunIdRef = useRef<string | null>(null);
  const [conn, setConn] = useState<StudioConnectionState>('connecting');
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [messages, setMessages] = useState<StudioChatMessage[]>([]);
  const [streaming, setStreaming] = useState('');
  const [busy, setBusy] = useState(false);
  const [entries, setEntries] = useState<WorkspaceEntry[]>([]);
  const [dirPath, setDirPath] = useState('');
  const [filePath, setFilePath] = useState<string | null>(null);
  const [fileContent, setFileContent] = useState('');
  const [fileDirty, setFileDirty] = useState(false);
  const [fileSaving, setFileSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [agentKind, setAgentKind] = useState<'core' | 'mock' | 'unknown'>('unknown');
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
        sessionIdRef.current = e.session_id;
        setActiveRunner(e.active_runner_device_id);
        setRole('controller');
        setMessages([]);
        streamingRef.current = '';
        streamedThisRunRef.current = false;
        activeRunIdRef.current = null;
        setStreaming('');
        setError(null);
        setBusy(false);
        lastSeqRef.current = env.seq ?? lastSeqRef.current;
        // Keep tree usable immediately after recovery.
        send({ type: 'workspace_list', path: dirPathRef.current || '' });
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
        sessionIdRef.current = e.session_id;
        setActiveRunner(e.active_runner_device_id);
        streamingRef.current = '';
        streamedThisRunRef.current = false;
        activeRunIdRef.current = null;
        setStreaming('');
        setError(null);
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
        streamingRef.current = '';
        streamedThisRunRef.current = false;
        activeRunIdRef.current = (ev as { run_id: string }).run_id ?? null;
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
        const delta = (ev as { delta: string }).delta ?? '';
        if (!delta) return;
        // Skip exact full-buffer re-emit (pre-tool content replay from host).
        if (
          streamingRef.current &&
          delta === streamingRef.current
        ) {
          return;
        }
        // Skip if delta is a pure prefix already held (rare reassembly).
        if (
          streamingRef.current &&
          streamingRef.current.startsWith(delta) &&
          delta.length < streamingRef.current.length
        ) {
          return;
        }
        streamedThisRunRef.current = true;
        // If host re-sends the whole segment after partial deltas, replace
        // rather than concatenate when delta already contains the buffer.
        if (
          streamingRef.current &&
          delta.startsWith(streamingRef.current) &&
          delta.length > streamingRef.current.length
        ) {
          streamingRef.current = delta;
        } else if (
          streamingRef.current &&
          delta.includes(streamingRef.current) &&
          delta.length > streamingRef.current.length
        ) {
          streamingRef.current = delta;
        } else {
          streamingRef.current += delta;
        }
        setStreaming(streamingRef.current);
        return;
      }
      if (t === 'message_completed') {
        const runId = (ev as { run_id: string }).run_id;
        const text = (ev as { text: string }).text ?? '';
        const pending = streamingRef.current;
        streamingRef.current = '';
        setStreaming('');
        setMessages((m) => {
          const next = [...m];
          if (pending.trim()) {
            next.push({
              id: reqId('a'),
              role: 'assistant',
              content: pending,
              runId,
            });
          }

          if (!text.trim()) return next;

          // Collect assistant segments already shown for this run. Never drop them.
          const prior = next
            .filter((x) => x.role === 'assistant' && x.runId === runId)
            .map((x) => x.content);
          const last = prior[prior.length - 1] ?? '';

          // Exact duplicate of last segment (or only pending) — skip.
          if (last === text || prior.some((p) => p === text)) {
            return next;
          }

          // Final response often concatenates the whole turn. Prefer the suffix
          // that is new relative to the joined prior segments.
          const joined = prior.join('');
          let toAdd = text;
          if (joined && text.startsWith(joined)) {
            toAdd = text.slice(joined.length);
          } else if (last && text.startsWith(last)) {
            toAdd = text.slice(last.length);
          } else if (joined && joined.includes(text)) {
            // Final text already fully visible as segments.
            return next;
          }

          if (toAdd.trim()) {
            next.push({
              id: reqId('a'),
              role: 'assistant',
              content: toAdd,
              runId,
            });
          }
          return next;
        });
        streamedThisRunRef.current = false;
        return;
      }
      if (t === 'tool_started') {
        const name = (ev as { name: string }).name;
        const callId = (ev as { call_id: string }).call_id;
        const runId = (ev as { run_id: string }).run_id;
        // Flush partial assistant text BEFORE the tool card so timeline is
        // message → tool → message, not one giant bubble with tools on top.
        const pending = streamingRef.current;
        streamingRef.current = '';
        setStreaming('');
        setMessages((m) => {
          const next = [...m];
          if (pending.trim()) {
            const last = next[next.length - 1];
            // Dedupe consecutive identical assistant segment for this run.
            if (
              !(
                last &&
                last.role === 'assistant' &&
                last.runId === runId &&
                last.content === pending
              )
            ) {
              next.push({
                id: reqId('a'),
                role: 'assistant',
                content: pending,
                runId,
              });
            }
          }
          // Avoid duplicate tool cards for the same call_id.
          if (callId && next.some((x) => x.role === 'tool' && x.callId === callId)) {
            return next;
          }
          next.push({
            id: callId || reqId('t'),
            role: 'tool',
            content: name,
            runId,
            callId,
            toolName: name,
            toolStatus: 'running',
          });
          return next;
        });
        return;
      }
      if (t === 'tool_completed') {
        const callId = (ev as { call_id: string }).call_id;
        const ok = (ev as { ok: boolean }).ok;
        setMessages((m) =>
          m.map((msg) =>
            msg.role === 'tool' && (msg.callId === callId || msg.id === callId)
              ? {
                  ...msg,
                  toolStatus: ok ? 'done' : 'error',
                  content: msg.toolName || msg.content,
                }
              : msg,
          ),
        );
        return;
      }
      if (t === 'session_ended') {
        // Flush any leftover stream at end of turn.
        const pending = streamingRef.current;
        streamingRef.current = '';
        setStreaming('');
        if (pending.trim()) {
          const runId =
            (ev as { run_id?: string }).run_id ?? activeRunIdRef.current ?? undefined;
          setMessages((m) => [
            ...m,
            { id: reqId('a'), role: 'assistant', content: pending, runId },
          ]);
        }
        setBusy(false);
        streamedThisRunRef.current = false;
        activeRunIdRef.current = null;
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
        const path = normalizeStudioPath(listed.path);
        setDirPath(path);
        setEntries(
          (listed.entries ?? []).map((e) => ({
            ...e,
            path: normalizeStudioPath(e.path),
          })),
        );
        pendingList.current?.(listed.entries ?? []);
        pendingList.current = null;
        return;
      }
      if (t === 'workspace_content') {
        const c = ev as { path: string; content: string };
        setFilePath(normalizeStudioPath(c.path) || c.path);
        setFileContent(c.content);
        setFileDirty(false);
        setFileSaving(false);
        pendingRead.current?.(c.content);
        pendingRead.current = null;
        return;
      }
      if (t === 'workspace_written') {
        const p = (ev as { path: string }).path;
        setFileSaving(false);
        setFileDirty(false);
        setMessages((m) => [
          ...m,
          {
            id: reqId('sys'),
            role: 'system',
            content: `Saved ${p}`,
          },
        ]);
        return;
      }
      if (t === 'workspace_created' || t === 'workspace_deleted') {
        const p = (ev as { path: string }).path;
        setMessages((m) => [
          ...m,
          {
            id: reqId('sys'),
            role: 'system',
            content: t === 'workspace_created' ? `Created ${p}` : `Deleted ${p}`,
          },
        ]);
        // Refresh current directory listing
        const dp = dirPathRef.current;
        const sid = sessionIdRef.current ?? undefined;
        send({ type: 'workspace_list', path: dp, session_id: sid });
        if (t === 'workspace_deleted' && filePathRef.current === p) {
          setFilePath(null);
          setFileContent('');
          setFileDirty(false);
        }
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
          setBusy(false);
          return;
        }
        // Server restarted / session expired: recreate and stay usable.
        if (
          code === 'session_not_found' ||
          /session not found/i.test(msg)
        ) {
          setSessionId(null);
          sessionIdRef.current = null;
          setBusy(false);
          setError('Session expired — creating a new one…');
          // Drop stale ?session= from URL so reconnect doesn't re-attach a ghost.
          try {
            const url = new URL(window.location.href);
            if (url.searchParams.has('session')) {
              url.searchParams.delete('session');
              url.searchParams.delete('role');
              window.history.replaceState({}, '', url.pathname + url.search + url.hash);
            }
          } catch {
            /* ignore */
          }
          send({
            type: 'session_create',
            title: 'Studio',
            prefer_runner: 'local',
          });
          return;
        }
        setError(msg);
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
        const attachSid = params.get('session') || sessionIdRef.current;
        const attachRole = (params.get('role') as AttachRole) || 'controller';
        if (attachSid) {
          setRole(attachRole);
          // Try resume; if server lost the session (restart), error handler
          // will session_create automatically.
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
            path: dirPathRef.current || '',
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
      const p = normalizeStudioPath(path);
      send({ type: 'workspace_list', path: p, session_id: sessionId ?? undefined });
    },
    [send, sessionId],
  );

  const readFile = useCallback(
    (path: string) => {
      const p = normalizeStudioPath(path);
      if (fileDirty && filePath && filePath !== p) {
        const ok = window.confirm(`Discard unsaved changes to ${filePath}?`);
        if (!ok) return;
      }
      setFileDirty(false);
      send({ type: 'workspace_read', path: p, session_id: sessionId ?? undefined });
    },
    [send, sessionId, fileDirty, filePath],
  );

  const setEditorContent = useCallback((content: string) => {
    setFileContent(content);
    setFileDirty(true);
  }, []);

  const saveFile = useCallback(() => {
    if (!filePath) return;
    if (role === 'viewer') {
      setError('Viewer mode — cannot save files');
      return;
    }
    setFileSaving(true);
    send({
      type: 'workspace_write',
      session_id: sessionId ?? undefined,
      path: filePath,
      content: fileContent,
    });
  }, [send, sessionId, filePath, fileContent, role]);

  const createPath = useCallback(
    (path: string, isDir: boolean) => {
      if (role === 'viewer') {
        setError('Viewer mode — cannot create files');
        return;
      }
      const trimmed = path.trim().replace(/^\/+/, '');
      if (!trimmed) return;
      send({
        type: 'workspace_create',
        session_id: sessionId ?? undefined,
        path: trimmed,
        is_dir: isDir,
      });
    },
    [send, sessionId, role],
  );

  const deletePath = useCallback(
    (path: string, isDir: boolean) => {
      if (role === 'viewer') {
        setError('Viewer mode — cannot delete');
        return;
      }
      send({
        type: 'workspace_delete',
        session_id: sessionId ?? undefined,
        path,
        recursive: isDir,
      });
    },
    [send, sessionId, role],
  );

  // Probe /v1/studio/health for agent kind (mock vs core)
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const r = await fetch('/v1/studio/health', { credentials: 'same-origin' });
        if (!r.ok) return;
        const j = (await r.json()) as { agent?: string };
        if (!cancelled && (j.agent === 'core' || j.agent === 'mock')) {
          setAgentKind(j.agent);
        }
      } catch {
        /* ignore */
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

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
        cwd: dirPath || undefined,
        focused_path: filePath || undefined,
      });
    },
    [send, sessionId, role, dirPath, filePath],
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
      streamingRef.current = '';
      streamedThisRunRef.current = false;
      activeRunIdRef.current = null;
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
    fileDirty,
    fileSaving,
    error,
    setError,
    agentKind,
    devices,
    sessions,
    checkpoints,
    role,
    activeRunner,
    deviceId,
    listDir,
    readFile,
    setEditorContent,
    saveFile,
    createPath,
    deletePath,
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
