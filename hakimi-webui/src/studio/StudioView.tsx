import {
  ChevronRight,
  FileText,
  Folder,
  History,
  Loader2,
  MonitorSmartphone,
  RefreshCcw,
  Send,
  SquareTerminal,
  Terminal,
  Users,
  Layers,
} from 'lucide-react';
import { useMemo, useState, type FormEvent } from 'react';
import hljs from '../highlighter';
import { useStudioClient } from './useStudioClient';
import StudioEcosystemPanel from './StudioEcosystemPanel';
import StudioCheckpointPanel from './StudioCheckpointPanel';
import { Danger } from './dangerConfirm';
import './studio.css';

const EXT_LANG: Record<string, string> = {
  ts: 'typescript',
  tsx: 'typescript',
  js: 'javascript',
  jsx: 'javascript',
  py: 'python',
  rs: 'rust',
  json: 'json',
  md: 'markdown',
  css: 'css',
  html: 'xml',
  sh: 'bash',
  yml: 'yaml',
  yaml: 'yaml',
  toml: 'ini',
  go: 'go',
};

function detectLang(name: string): string | null {
  const ext = name.split('.').pop()?.toLowerCase() ?? '';
  return EXT_LANG[ext] ?? null;
}

/**
 * Hakimi Studio IDE: file tree | editor | agent chat + multi-device strip.
 */
export default function StudioView() {
  const studio = useStudioClient();
  const [draft, setDraft] = useState('');
  const [showDevices, setShowDevices] = useState(true);
  const [showEcosystem, setShowEcosystem] = useState(false);
  const [showCheckpoints, setShowCheckpoints] = useState(false);

  const segments = useMemo(
    () => (studio.dirPath ? studio.dirPath.split('/').filter(Boolean) : []),
    [studio.dirPath],
  );

  const highlighted = useMemo(() => {
    if (!studio.filePath || studio.fileContent.length > 300_000) return '';
    const lang = detectLang(studio.filePath);
    try {
      if (lang && hljs.getLanguage(lang)) {
        return hljs.highlight(studio.fileContent, { language: lang }).value;
      }
      return hljs.highlightAuto(studio.fileContent).value;
    } catch {
      return '';
    }
  }, [studio.fileContent, studio.filePath]);

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    const text = draft.trim();
    if (!text) return;
    studio.submit(text, false);
    setDraft('');
  }

  const connLabel =
    studio.conn === 'open'
      ? 'connected'
      : studio.conn === 'connecting'
        ? 'connecting…'
        : studio.conn;

  const canSubmit = studio.role === 'controller' && studio.conn === 'open';

  return (
    <div className="studio-root">
      <div className="studio-status">
        <span className={`studio-live ${studio.conn === 'open' ? 'is-on' : ''}`}>
          <Terminal size={12} aria-hidden />
          Studio · {connLabel}
        </span>
        {studio.sessionId && (
          <span className="studio-session mono" title={studio.sessionId}>
            {studio.sessionId.slice(0, 16)}…
          </span>
        )}
        <span className={`studio-role role-${studio.role}`}>{studio.role}</span>
        {studio.activeRunner && (
          <span className="studio-runner mono" title={studio.activeRunner}>
            runner {studio.activeRunner.slice(0, 12)}
          </span>
        )}
        {studio.busy && <span className="studio-busy">running</span>}
        <button
          type="button"
          className="studio-btn ghost compact"
          onClick={() => {
            setShowDevices((v) => !v);
            studio.refreshDevices();
            studio.refreshSessions();
          }}
          title="Devices & sessions"
        >
          <Users size={12} /> Devices
        </button>
        <button
          type="button"
          className="studio-btn ghost compact"
          onClick={() => setShowEcosystem((v) => !v)}
          title="Fleet / Skills / MCP"
        >
          <Layers size={12} /> Hub
        </button>
        <button
          type="button"
          className="studio-btn ghost compact"
          onClick={() => {
            setShowCheckpoints((v) => !v);
            studio.refreshCheckpoints();
          }}
          title="Checkpoints / Rewind"
        >
          <History size={12} /> CP
        </button>
        {studio.error && (
          <button
            type="button"
            className="studio-error"
            onClick={() => studio.setError(null)}
            title="Dismiss"
          >
            {studio.error}
          </button>
        )}
      </div>

      {showDevices && (
        <div className="studio-multi">
          <div className="studio-multi-col">
            <div className="studio-multi-head">
              <MonitorSmartphone size={12} /> Devices
              <button
                type="button"
                className="studio-btn ghost compact"
                onClick={() => studio.refreshDevices()}
              >
                <RefreshCcw size={12} />
              </button>
            </div>
            <div className="studio-multi-list">
              {studio.devices.length === 0 && (
                <div className="studio-empty tiny">No devices yet</div>
              )}
              {studio.devices.map((d) => {
                const mine = d.device_id === studio.deviceId;
                return (
                  <div
                    key={d.device_id}
                    className={`studio-device ${mine ? 'is-me' : ''}`}
                  >
                    <span className="mono" title={d.device_id}>
                      {d.device_name || d.device_id.slice(0, 14)}
                      {mine ? ' · you' : ''}
                    </span>
                    <span className="studio-device-kind">{d.kind}</span>
                    {!mine && studio.role === 'controller' && studio.sessionId && (
                      <button
                        type="button"
                        className="studio-btn ghost compact"
                        onClick={() => {
                          if (Danger.handoff(d.device_name || d.device_id)) {
                            studio.handoffTo(d.device_id);
                          }
                        }}
                        title="Handoff Active Runner"
                      >
                        Handoff
                      </button>
                    )}
                  </div>
                );
              })}
            </div>
          </div>
          <div className="studio-multi-col">
            <div className="studio-multi-head">
              Sessions
              <button
                type="button"
                className="studio-btn ghost compact"
                onClick={() => studio.refreshSessions()}
              >
                <RefreshCcw size={12} />
              </button>
            </div>
            <div className="studio-multi-list">
              {studio.sessions.length === 0 && (
                <div className="studio-empty tiny">No sessions listed</div>
              )}
              {studio.sessions.map((s) => (
                <div
                  key={s.session_id}
                  className={`studio-session-row ${
                    s.session_id === studio.sessionId ? 'is-active' : ''
                  }`}
                >
                  <span className="mono" title={s.session_id}>
                    {s.title || s.session_id.slice(0, 12)}
                  </span>
                  <button
                    type="button"
                    className="studio-btn ghost compact"
                    onClick={() => studio.attachSession(s.session_id, 'viewer')}
                  >
                    View
                  </button>
                  <button
                    type="button"
                    className="studio-btn ghost compact"
                    onClick={() => studio.attachSession(s.session_id, 'controller')}
                  >
                    Control
                  </button>
                </div>
              ))}
            </div>
          </div>
          <div className="studio-multi-actions">
            <button
              type="button"
              className="studio-btn ghost compact"
              disabled={!studio.sessionId}
              onClick={() => studio.becomeController()}
            >
              Be controller
            </button>
            <button
              type="button"
              className="studio-btn ghost compact"
              disabled={!studio.sessionId}
              onClick={() => studio.becomeViewer()}
            >
              Be viewer
            </button>
            <span className="studio-device-id mono" title={studio.deviceId}>
              me {studio.deviceId.slice(0, 16)}
            </span>
          </div>
        </div>
      )}

      <StudioEcosystemPanel open={showEcosystem} />
      <StudioCheckpointPanel
        open={showCheckpoints}
        checkpoints={studio.checkpoints}
        canMutate={studio.role === 'controller' && studio.conn === 'open'}
        onRefresh={() => studio.refreshCheckpoints()}
        onCreate={(label) => studio.createCheckpoint(label)}
        onRestore={(id) => studio.restoreCheckpoint(id)}
      />

      <div className="studio-panels">
        <aside className="studio-tree" aria-label="Workspace tree">
          <div className="studio-tree-head">
            <nav className="studio-crumbs">
              <button type="button" onClick={() => studio.listDir('')}>
                /
              </button>
              {segments.map((seg, i) => (
                <span key={`${seg}-${i}`}>
                  <ChevronRight size={12} aria-hidden />
                  <button
                    type="button"
                    onClick={() => studio.listDir(segments.slice(0, i + 1).join('/'))}
                  >
                    {seg}
                  </button>
                </span>
              ))}
            </nav>
          </div>
          <div className="studio-tree-body">
            {studio.entries.length === 0 && <div className="studio-empty">Empty</div>}
            {studio.entries.map((entry) => {
              const active = !entry.is_dir && studio.filePath === entry.path;
              return (
                <button
                  key={entry.path}
                  type="button"
                  className={`studio-entry ${active ? 'is-active' : ''}`}
                  onClick={() => {
                    if (entry.is_dir) studio.listDir(entry.path);
                    else studio.readFile(entry.path);
                  }}
                >
                  {entry.is_dir ? (
                    <Folder size={14} aria-hidden />
                  ) : (
                    <FileText size={14} aria-hidden />
                  )}
                  <span>{entry.name}</span>
                </button>
              );
            })}
          </div>
        </aside>

        <section className="studio-editor" aria-label="Editor">
          {studio.filePath ? (
            <>
              <div className="studio-file-head mono">{studio.filePath}</div>
              <pre className="studio-code">
                {highlighted ? (
                  <code className="hljs" dangerouslySetInnerHTML={{ __html: highlighted }} />
                ) : (
                  <code className="hljs">{studio.fileContent}</code>
                )}
              </pre>
            </>
          ) : (
            <div className="studio-editor-empty">
              <FileText size={28} aria-hidden />
              <h3>Hakimi Studio</h3>
              <p>Select a file. Multi-device: share ?session=&role=viewer</p>
            </div>
          )}
        </section>

        <section className="studio-chat" aria-label="Agent chat">
          <div className="studio-chat-head">
            <SquareTerminal size={14} aria-hidden />
            Agent
            {studio.busy && <Loader2 size={14} className="spin" aria-hidden />}
            {studio.role === 'viewer' && <span className="studio-badge">read-only</span>}
          </div>
          <div className="studio-chat-body">
            {studio.messages.length === 0 && !studio.streaming && (
              <div className="studio-empty">
                Ask the agent. Controller can submit; viewers observe.
              </div>
            )}
            {studio.messages.map((m) => (
              <div key={m.id} className={`studio-msg role-${m.role}`}>
                <div className="studio-msg-role">{m.role}</div>
                <div className="studio-msg-body">{m.content}</div>
              </div>
            ))}
            {studio.streaming && (
              <div className="studio-msg role-assistant is-stream">
                <div className="studio-msg-role">assistant</div>
                <div className="studio-msg-body">{studio.streaming}</div>
              </div>
            )}
          </div>
          <form className="studio-composer" onSubmit={onSubmit}>
            <textarea
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              placeholder={
                canSubmit
                  ? 'Message Studio agent…'
                  : studio.role === 'viewer'
                    ? 'Viewer mode — watch only'
                    : 'Connecting…'
              }
              rows={2}
              disabled={!canSubmit}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && !e.shiftKey) {
                  e.preventDefault();
                  onSubmit(e);
                }
              }}
            />
            <div className="studio-composer-actions">
              {studio.busy && canSubmit && (
                <button type="button" className="studio-btn ghost" onClick={() => studio.cancel()}>
                  Stop
                </button>
              )}
              {studio.busy && canSubmit && (
                <button
                  type="button"
                  className="studio-btn ghost"
                  onClick={() => {
                    const t = draft.trim();
                    if (!t) return;
                    studio.submit(t, true);
                    setDraft('');
                  }}
                  title="Preempt current run"
                >
                  Preempt
                </button>
              )}
              <button
                type="submit"
                className="studio-btn primary"
                disabled={!draft.trim() || !canSubmit}
              >
                <Send size={14} /> Send
              </button>
            </div>
          </form>
        </section>
      </div>
    </div>
  );
}
