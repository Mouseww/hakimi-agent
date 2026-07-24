import {
  ChevronRight,
  FileText,
  Folder,
  History,
  Loader2,
  MonitorSmartphone,
  Plus,
  RefreshCcw,
  Save,
  Send,
  SquareTerminal,
  Terminal,
  Trash2,
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
  const [preview, setPreview] = useState(false);

  const segments = useMemo(() => {
    const p = studio.dirPath.replace(/\\/g, '/').replace(/^\/+|\/+$/g, '');
    return p ? p.split('/').filter(Boolean) : [];
  }, [studio.dirPath]);

  const pathLabel = useMemo(() => {
    if (!studio.dirPath) return 'workspace';
    return studio.dirPath.replace(/\\/g, '/');
  }, [studio.dirPath]);

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

  function joinPath(base: string, name: string): string {
    const b = base.replace(/\/+$/, '');
    const n = name.replace(/^\/+/, '');
    return b ? `${b}/${n}` : n;
  }

  function onNewFile() {
    const name = window.prompt(
      'New file path (relative to workspace root)',
      joinPath(studio.dirPath, 'untitled.txt'),
    );
    if (!name) return;
    studio.createPath(name, false);
  }

  function onNewFolder() {
    const name = window.prompt('New folder path', joinPath(studio.dirPath, 'new-folder'));
    if (!name) return;
    studio.createPath(name, true);
  }

  const connLabel =
    studio.conn === 'open'
      ? 'connected'
      : studio.conn === 'connecting'
        ? 'connecting…'
        : studio.conn;

  const canSubmit = studio.role === 'controller' && studio.conn === 'open';
  const canEdit = canSubmit;

  return (
    <div className="studio-root">
      <div className="studio-status">
        <span className={`studio-live ${studio.conn === 'open' ? 'is-on' : ''}`}>
          <Terminal size={12} aria-hidden />
          Studio · {connLabel}
        </span>
        <span
          className={`studio-agent-badge agent-${studio.agentKind}`}
          title={
            studio.agentKind === 'mock'
              ? 'MockAgentHost — echo only. Use hakimi --serve for real AIAgent.'
              : studio.agentKind === 'core'
                ? 'CoreAgentHost — real AIAgent + tools'
                : 'Probing agent host…'
          }
        >
          agent:{studio.agentKind}
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

      {studio.agentKind === 'mock' && (
        <div className="studio-banner mock">
          当前是 <strong>Mock Agent</strong>（回声测试），不是真实模型。请用{' '}
          <code>hakimi --serve</code> / 统一 Gateway 启动（会注入 CoreAgentHost），不要用仅桌面
          mock 壳。
        </div>
      )}

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
                  <div key={d.device_id} className={`studio-device ${mine ? 'is-me' : ''}`}>
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
            <nav className="studio-crumbs" title={pathLabel === 'workspace' ? '/' : `/${pathLabel}`}>
              <button type="button" className="crumb-root" onClick={() => studio.listDir('')}>
                workspace
              </button>
              {segments.map((seg, i) => {
                const target = segments.slice(0, i + 1).join('/');
                const isLast = i === segments.length - 1;
                return (
                  <span key={`${target}-${i}`} className="crumb-seg">
                    <ChevronRight size={11} aria-hidden className="crumb-sep" />
                    <button
                      type="button"
                      className={isLast ? 'is-current' : undefined}
                      onClick={() => studio.listDir(target)}
                      title={target}
                    >
                      {seg}
                    </button>
                  </span>
                );
              })}
            </nav>
            <div className="studio-tree-actions">
              <button
                type="button"
                className="studio-btn ghost compact"
                disabled={!canEdit}
                onClick={onNewFile}
                title="New file"
              >
                <Plus size={12} />
                <FileText size={12} />
              </button>
              <button
                type="button"
                className="studio-btn ghost compact"
                disabled={!canEdit}
                onClick={onNewFolder}
                title="New folder"
              >
                <Plus size={12} />
                <Folder size={12} />
              </button>
              <button
                type="button"
                className="studio-btn ghost compact"
                onClick={() => studio.listDir(studio.dirPath)}
                title="Refresh"
              >
                <RefreshCcw size={12} />
              </button>
            </div>
          </div>
          <div className="studio-tree-body">
            {studio.entries.length === 0 && <div className="studio-empty">Empty</div>}
            {studio.entries.map((entry) => {
              const active = !entry.is_dir && studio.filePath === entry.path;
              return (
                <div
                  key={entry.path}
                  className={`studio-entry-row ${active ? 'is-active' : ''}`}
                >
                  <button
                    type="button"
                    className="studio-entry"
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
                  {canEdit && (
                    <button
                      type="button"
                      className="studio-btn ghost compact entry-del"
                      title="Delete"
                      onClick={() => {
                        const ok = entry.is_dir
                          ? Danger.deleteRecursive(entry.path)
                          : Danger.deleteFile(entry.path);
                        if (ok) studio.deletePath(entry.path, entry.is_dir);
                      }}
                    >
                      <Trash2 size={12} />
                    </button>
                  )}
                </div>
              );
            })}
          </div>
        </aside>

        <section className="studio-editor" aria-label="Editor">
          {studio.filePath ? (
            <>
              <div className="studio-file-head mono">
                <span className="studio-file-path">
                  {studio.filePath}
                  {studio.fileDirty ? ' ·' : ''}
                </span>
                <div className="studio-file-actions">
                  <button
                    type="button"
                    className="studio-btn ghost compact"
                    onClick={() => setPreview((v) => !v)}
                    title="Toggle highlight preview"
                  >
                    {preview ? 'Edit' : 'Preview'}
                  </button>
                  <button
                    type="button"
                    className="studio-btn primary compact"
                    disabled={!canEdit || !studio.fileDirty || studio.fileSaving}
                    onClick={() => studio.saveFile()}
                    title="Save (Ctrl/Cmd+S)"
                  >
                    {studio.fileSaving ? (
                      <Loader2 size={12} className="spin" />
                    ) : (
                      <Save size={12} />
                    )}{' '}
                    Save
                  </button>
                </div>
              </div>
              {preview ? (
                <pre className="studio-code">
                  {highlighted ? (
                    <code className="hljs" dangerouslySetInnerHTML={{ __html: highlighted }} />
                  ) : (
                    <code className="hljs">{studio.fileContent}</code>
                  )}
                </pre>
              ) : (
                <textarea
                  className="studio-code-edit"
                  value={studio.fileContent}
                  disabled={!canEdit}
                  spellCheck={false}
                  onChange={(e) => studio.setEditorContent(e.target.value)}
                  onKeyDown={(e) => {
                    if ((e.metaKey || e.ctrlKey) && e.key === 's') {
                      e.preventDefault();
                      if (canEdit && studio.fileDirty) studio.saveFile();
                    }
                  }}
                />
              )}
            </>
          ) : (
            <div className="studio-editor-empty">
              <FileText size={28} aria-hidden />
              <h3>Hakimi Studio</h3>
              <p>
                Open a file to edit. Use tree actions for New / Delete. Multi-device: share{' '}
                <code>?session=&role=viewer</code>
              </p>
            </div>
          )}
        </section>

        <section className="studio-chat" aria-label="Agent chat">
          <div className="studio-chat-head">
            <SquareTerminal size={14} aria-hidden />
            Agent
            {studio.busy && <Loader2 size={14} className="spin" aria-hidden />}
            {studio.role === 'viewer' && <span className="studio-badge">read-only</span>}
            {studio.agentKind === 'mock' && <span className="studio-badge mock">mock</span>}
          </div>
          <div className="studio-chat-body">
            {studio.messages.length === 0 && !studio.streaming && (
              <div className="studio-empty">
                {studio.agentKind === 'mock'
                  ? 'Mock host: messages are echoed. Start with hakimi --serve for real agent.'
                  : 'Ask the agent. Controller can submit; viewers observe.'}
              </div>
            )}
            {studio.messages.map((m) => (
              <div
                key={m.id}
                className={`studio-msg role-${m.role}${
                  m.role === 'tool' && m.toolStatus ? ` tool-${m.toolStatus}` : ''
                }`}
              >
                <div className="studio-msg-role">
                  {m.role === 'tool'
                    ? m.toolStatus === 'running'
                      ? 'tool · running'
                      : m.toolStatus === 'error'
                        ? 'tool · error'
                        : 'tool'
                    : m.role}
                </div>
                <div className="studio-msg-body">
                  {m.role === 'tool' ? (
                    <span className="studio-tool-line">
                      <span className="studio-tool-icon" aria-hidden>
                        {m.toolStatus === 'running' ? '◉' : m.toolStatus === 'error' ? '✕' : '✓'}
                      </span>
                      <code>{m.toolName || m.content}</code>
                    </span>
                  ) : (
                    m.content
                  )}
                </div>
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
                  ? studio.agentKind === 'mock'
                    ? 'Mock mode — will only echo…'
                    : 'Message Studio agent…'
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
