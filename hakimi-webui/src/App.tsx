import {
  Activity,
  BadgeCheck,
  Bot,
  Boxes,
  Brain,
  Copy,
  Database,
  FileSearch,
  Gauge,
  KeyRound,
  Layers3,
  Loader2,
  MessageSquare,
  PanelLeft,
  PanelRight,
  RefreshCcw,
  RotateCcw,
  Search,
  Send,
  Server,
  ShieldCheck,
  SquareTerminal,
  Trash2,
  Workflow,
  Wrench,
} from 'lucide-react';
import { useEffect, useMemo, useState, type FormEvent } from 'react';
import './App.css';
import MessageContent from './MessageContent';
import PersonaRail from './PersonaRail';
import PersonaConfigForm from './PersonaConfigForm';
import InstanceSettings from './InstanceSettings';
import WorkspacePanel from './WorkspacePanel';
import OfficeView from './OfficeView';
import {
  api,
  getAuthToken,
  setAuthToken,
  type Agent,
  type CapabilitiesResponse,
  type ChatResponse,
  type CredentialPoolResponse,
  type DashboardStatus,
  type HealthResponse,
  type McpServersResponse,
  type SessionInfo,
  type SessionMessageInfo,
  type SkillInfo,
  type ToolInfo,
  type ToolsetInfo,
  type WebhookResponse,
} from './api';

type RightPanel = 'runtime' | 'tools' | 'skills';

type UiMessage = {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  sessionId?: string;
  createdAt: Date;
};

type LoadState = {
  health: HealthResponse | null;
  status: DashboardStatus | null;
  capabilities: CapabilitiesResponse | null;
  sessions: SessionInfo[];
  tools: ToolInfo[];
  skills: SkillInfo[];
  toolsets: ToolsetInfo[];
  mcp: McpServersResponse | null;
  credentials: CredentialPoolResponse | null;
  webhooks: WebhookResponse | null;
};

const emptyState: LoadState = {
  health: null,
  status: null,
  capabilities: null,
  sessions: [],
  tools: [],
  skills: [],
  toolsets: [],
  mcp: null,
  credentials: null,
  webhooks: null,
};

function nowId(prefix: string): string {
  return `${prefix}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function formatDate(value: string | null): string {
  if (!value) {
    return 'pending';
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(date);
}

function compactNumber(value: number): string {
  return new Intl.NumberFormat(undefined, { notation: 'compact' }).format(value);
}

function sessionLabel(session: SessionInfo): string {
  return session.title || session.id;
}

function roleLabel(role: string): string {
  if (role === 'assistant') {
    return 'assistant';
  }
  if (role === 'tool') {
    return 'tool';
  }
  return 'user';
}

function featureValue(value: boolean | string): string {
  if (typeof value === 'boolean') {
    return value ? 'enabled' : 'off';
  }
  return value;
}

function pickTopFeatures(capabilities: CapabilitiesResponse | null): Array<[string, boolean | string]> {
  if (!capabilities) {
    return [];
  }

  return Object.entries(capabilities.features)
    .filter(([name]) =>
      [
        'chat',
        'chat_completions',
        'responses_api',
        'skills_api',
        'toolsets_api',
        'session_messages',
        'session_search',
        'run_events_sse',
      ].includes(name),
    )
    .slice(0, 8);
}

function App() {
  const [data, setData] = useState<LoadState>(emptyState);
  const [rightPanel, setRightPanel] = useState<RightPanel>('runtime');
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [authDraft, setAuthDraft] = useState(getAuthToken());
  const [composer, setComposer] = useState('');
  const [sending, setSending] = useState(false);
  const [messages, setMessages] = useState<UiMessage[]>([]);
  const [selectedSessionId, setSelectedSessionId] = useState<string | null>(null);
  const [sessionMessages, setSessionMessages] = useState<SessionMessageInfo[]>([]);
  const [sessionLoading, setSessionLoading] = useState(false);
  const [sessionQuery, setSessionQuery] = useState('');
  const [toolQuery, setToolQuery] = useState('');
  const [agents, setAgents] = useState<Agent[]>([]);
  const [activePersonaId, setActivePersonaId] = useState<string | null>(null);
  const [view, setView] = useState<'chat' | 'config' | 'instance' | 'workspace' | 'office'>('chat');
  const [editingPersona, setEditingPersona] = useState<Agent | null>(null);
  const [showSessions, setShowSessions] = useState(true);
  const [showPanel, setShowPanel] = useState(true);

  const activePersona = useMemo(
    () => agents.find((a) => a.id === activePersonaId) ?? null,
    [agents, activePersonaId],
  );
  const availableSkillNames = useMemo(() => data.skills.map((s) => s.name), [data.skills]);

  // What the center transcript renders: the live exchange when present, otherwise
  // the selected session's stored conversation (so clicking a session shows it).
  const transcriptMessages = useMemo<UiMessage[]>(() => {
    if (messages.length > 0) {
      return messages;
    }
    return sessionMessages
      .filter(
        (m) => (m.role === 'user' || m.role === 'assistant') && (m.content ?? '').trim().length > 0,
      )
      .map((m, index) => ({
        id: `session-${selectedSessionId ?? 'none'}-${index}`,
        role: m.role === 'assistant' ? 'assistant' : 'user',
        content: m.content ?? '',
        sessionId: selectedSessionId ?? undefined,
        createdAt: m.timestamp ? new Date(m.timestamp) : new Date(),
      }));
  }, [messages, sessionMessages, selectedSessionId]);

  async function loadAgents() {
    try {
      const res = await api.agents();
      setAgents(res.agents);
      setActivePersonaId((current) => current ?? res.default);
    } catch {
      // agents endpoint is optional; keep chat working against the default persona
    }
  }

  const selectedSession = useMemo(
    () => data.sessions.find((session) => session.id === selectedSessionId) ?? null,
    [data.sessions, selectedSessionId],
  );

  const visibleSessions = useMemo(() => {
    const query = sessionQuery.trim().toLowerCase();
    if (!query) {
      return data.sessions;
    }
    return data.sessions.filter((session) => {
      const text = [
        session.id,
        session.title,
        session.model,
        session.source,
        session.user_id,
      ]
        .filter(Boolean)
        .join(' ')
        .toLowerCase();
      return text.includes(query);
    });
  }, [data.sessions, sessionQuery]);

  const visibleTools = useMemo(() => {
    const query = toolQuery.trim().toLowerCase();
    if (!query) {
      return data.tools;
    }
    return data.tools.filter((tool) =>
      `${tool.name} ${tool.description}`.toLowerCase().includes(query),
    );
  }, [data.tools, toolQuery]);

  const activeSkills = useMemo(
    () => data.skills.filter((skill) => skill.active),
    [data.skills],
  );

  async function refreshAll(options: { quiet?: boolean } = {}) {
    if (options.quiet) {
      setRefreshing(true);
    } else {
      setLoading(true);
    }
    setError(null);

    const [
      health,
      status,
      capabilities,
      sessions,
      tools,
      skills,
      toolsets,
      mcp,
      credentials,
      webhooks,
    ] = await Promise.allSettled([
      api.health(),
      api.status(),
      api.capabilities(),
      api.sessions(),
      api.tools(),
      api.skills(),
      api.toolsets(),
      api.mcpServers(),
      api.credentialPools(),
      api.webhooks(),
    ]);

    const nextData: LoadState = {
      health: health.status === 'fulfilled' ? health.value : null,
      status: status.status === 'fulfilled' ? status.value : null,
      capabilities: capabilities.status === 'fulfilled' ? capabilities.value : null,
      sessions: sessions.status === 'fulfilled' ? sessions.value : [],
      tools: tools.status === 'fulfilled' ? tools.value : [],
      skills: skills.status === 'fulfilled' ? skills.value.data : [],
      toolsets: toolsets.status === 'fulfilled' ? toolsets.value.data : [],
      mcp: mcp.status === 'fulfilled' ? mcp.value : null,
      credentials: credentials.status === 'fulfilled' ? credentials.value : null,
      webhooks: webhooks.status === 'fulfilled' ? webhooks.value : null,
    };

    const firstFailure = [
      status,
      capabilities,
      sessions,
      tools,
      skills,
      toolsets,
      mcp,
      credentials,
      webhooks,
    ].find((result) => result.status === 'rejected');

    if (firstFailure?.status === 'rejected') {
      setError(firstFailure.reason instanceof Error ? firstFailure.reason.message : String(firstFailure.reason));
    }

    setData(nextData);
    setLoading(false);
    setRefreshing(false);
  }

  async function loadSessionMessages(sessionId: string) {
    setSelectedSessionId(sessionId);
    setSessionLoading(true);
    // Clear the live transcript so the selected session's conversation is what
    // the center renders (see `transcriptMessages`).
    setMessages([]);

    try {
      const response = await api.sessionMessages(sessionId);
      setSessionMessages(response.messages);
    } catch (loadError) {
      setSessionMessages([]);
      setError(loadError instanceof Error ? loadError.message : String(loadError));
    } finally {
      setSessionLoading(false);
    }
  }

  async function runTurn(content: string) {
    const text = content.trim();
    if (!text || sending) {
      return;
    }

    const userMessage: UiMessage = {
      id: nowId('user'),
      role: 'user',
      content: text,
      createdAt: new Date(),
    };
    setMessages((current) => [...current, userMessage]);
    setSending(true);
    setError(null);

    const assistantId = nowId('assistant');
    setMessages((current) => [
      ...current,
      { id: assistantId, role: 'assistant', content: '', createdAt: new Date() },
    ]);
    const applyContent = (value: string) =>
      setMessages((current) =>
        current.map((message) =>
          message.id === assistantId ? { ...message, content: value } : message,
        ),
      );

    try {
      let response: ChatResponse;
      if (activePersonaId) {
        // Stream tokens live for persona chat. Pass selectedSessionId to continue
        // the same conversation across turns; the backend creates a new session if null.
        let accumulated = '';
        response = await api.agentChatStream(activePersonaId, text, {
          sessionId: selectedSessionId ?? undefined,
          onToken: (token) => {
            accumulated += token;
            applyContent(accumulated);
          },
        });
      } else {
        response = await api.chat(text);
      }
      setMessages((current) =>
        current.map((message) =>
          message.id === assistantId
            ? { ...message, content: response.response, sessionId: response.session_id }
            : message,
        ),
      );
      setSelectedSessionId(response.session_id);
      void refreshAll({ quiet: true });
    } catch (sendError) {
      setMessages((current) => current.filter((message) => message.id !== assistantId));
      setError(sendError instanceof Error ? sendError.message : String(sendError));
    } finally {
      setSending(false);
    }
  }

  async function sendMessage(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const content = composer.trim();
    if (!content || sending) {
      return;
    }
    setComposer('');
    await runTurn(content);
  }

  function copyMessage(content: string) {
    void navigator.clipboard?.writeText(content);
  }

  function retryMessage(message: UiMessage) {
    if (sending) {
      return;
    }
    let userText = message.content;
    if (message.role === 'assistant') {
      const index = transcriptMessages.findIndex((m) => m.id === message.id);
      for (let i = index - 1; i >= 0; i -= 1) {
        if (transcriptMessages[i].role === 'user') {
          userText = transcriptMessages[i].content;
          break;
        }
      }
    }
    void runTurn(userText);
  }

  function deleteMessage(id: string) {
    setMessages((current) => current.filter((message) => message.id !== id));
  }

  async function handleDeleteSession(sessionId: string) {
    if (!window.confirm('Delete this session? This cannot be undone.')) {
      return;
    }
    try {
      await api.deleteSession(sessionId);
      setSelectedSessionId((current) => (current === sessionId ? null : current));
      if (selectedSessionId === sessionId) {
        setSessionMessages([]);
        setMessages([]);
      }
      void refreshAll({ quiet: true });
    } catch (deleteError) {
      setError(deleteError instanceof Error ? deleteError.message : String(deleteError));
    }
  }

  function saveAuthToken() {
    setAuthToken(authDraft);
    void refreshAll({ quiet: true });
  }

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void refreshAll();
      void loadAgents();
    }, 0);

    return () => {
      window.clearTimeout(timer);
    };
  }, []);

  function handleSelectPersona(id: string) {
    setActivePersonaId(id);
    setView('chat');
    // Start fresh in the new persona's context so a previous persona's session
    // transcript doesn't linger in the center.
    setMessages([]);
    setSessionMessages([]);
    setSelectedSessionId(null);
  }
  function handleEditPersona(id: string) {
    setEditingPersona(agents.find((a) => a.id === id) ?? null);
    setView('config');
  }
  function handleCreatePersona() {
    setEditingPersona(null);
    setView('config');
  }
  function handlePersonaSaved(saved: Agent) {
    setAgents((current) => {
      const exists = current.some((a) => a.id === saved.id);
      const next = exists
        ? current.map((a) => (a.id === saved.id ? saved : a))
        : [...current, saved];
      return saved.is_default
        ? next.map((a) => (a.id === saved.id ? a : { ...a, is_default: false }))
        : next;
    });
    setActivePersonaId(saved.id);
    setView('chat');
    void loadAgents();
  }
  function handlePersonaDeleted(id: string) {
    setAgents((current) => current.filter((a) => a.id !== id));
    setActivePersonaId((current) => (current === id ? null : current));
    setView('chat');
    void loadAgents();
  }

  const topFeatures = pickTopFeatures(data.capabilities);
  const sampledSessions = data.status?.resources.sessions_sampled ?? data.sessions.length;
  const totalTokens = data.sessions.reduce(
    (sum, session) => sum + session.input_tokens + session.output_tokens,
    0,
  );

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand-lockup">
          <div className="brand-mark" aria-hidden="true">
            H
          </div>
          <div>
            <p className="eyebrow">Hakimi Agent</p>
            <h1>Operator Console</h1>
          </div>
        </div>

        <div className="topbar-status">
          <span className={`live-dot ${data.health?.status === 'ok' ? 'is-live' : ''}`} />
          <span>{data.health?.status === 'ok' ? `v${data.health.version}` : 'offline'}</span>
          <span className="topbar-divider" />
          <span>{data.status?.model ?? 'model pending'}</span>
        </div>

        <div className="auth-cluster">
          <KeyRound size={16} aria-hidden="true" />
          <input
            aria-label="Bearer token"
            type="password"
            value={authDraft}
            onChange={(event) => setAuthDraft(event.target.value)}
            placeholder="Bearer token"
          />
          <button className="icon-button" type="button" onClick={saveAuthToken} title="Save token">
            <ShieldCheck size={16} aria-hidden="true" />
          </button>
          <button
            className="icon-button"
            type="button"
            onClick={() => void refreshAll({ quiet: true })}
            disabled={refreshing}
            title="Refresh"
          >
            {refreshing ? <Loader2 className="spin" size={16} aria-hidden="true" /> : <RefreshCcw size={16} aria-hidden="true" />}
          </button>
        </div>
      </header>

      <div className="console-body">
        <PersonaRail
          agents={agents}
          activeId={activePersonaId}
          view={view}
          onSelect={handleSelectPersona}
          onEdit={handleEditPersona}
          onCreate={handleCreatePersona}
          onInstance={() => setView('instance')}
          onWorkspace={() => setView('workspace')}
          onOffice={() => setView('office')}
        />
        <div className="console-main">
          {view === 'office' ? (
            <OfficeView onOpenPersona={handleSelectPersona} />
          ) : view === 'instance' ? (
            <InstanceSettings />
          ) : view === 'workspace' ? (
            <WorkspacePanel />
          ) : view === 'config' ? (
            <PersonaConfigForm
              agent={editingPersona}
              availableSkills={availableSkillNames}
              onSaved={handlePersonaSaved}
              onDeleted={handlePersonaDeleted}
              onCancel={() => setView('chat')}
            />
          ) : (
            <div
              className={`workspace-grid ${showSessions ? '' : 'sessions-collapsed'} ${showPanel ? '' : 'panel-collapsed'}`}
            >
        <aside className="left-rail">
          <section className="rail-section rail-section-metrics" aria-label="Runtime summary">
            <div className="metric-strip">
              <div>
                <span>{sampledSessions}</span>
                <small>sessions</small>
              </div>
              <div>
                <span>{data.tools.length}</span>
                <small>tools</small>
              </div>
              <div>
                <span>{compactNumber(totalTokens)}</span>
                <small>tokens</small>
              </div>
            </div>
          </section>

          <section className="rail-section">
            <div className="rail-heading">
              <div>
                <p className="eyebrow">Sessions</p>
                <h2>Recent Work</h2>
              </div>
              <Database size={18} aria-hidden="true" />
            </div>
            <div className="search-field">
              <Search size={15} aria-hidden="true" />
              <input
                value={sessionQuery}
                onChange={(event) => setSessionQuery(event.target.value)}
                placeholder="Filter sessions"
              />
            </div>
            <div className="session-list">
              {visibleSessions.map((session) => (
                <div className="session-row-wrap" key={session.id}>
                  <button
                    className={`session-row ${selectedSessionId === session.id ? 'is-active' : ''}`}
                    type="button"
                    onClick={() => void loadSessionMessages(session.id)}
                  >
                    <span className="session-title">{sessionLabel(session)}</span>
                    <span className="session-meta">
                      {session.message_count} msg / {session.tool_call_count} tools
                    </span>
                    <span className="session-foot">
                      <span>{session.model ?? 'model unknown'}</span>
                      <span>{formatDate(session.started_at)}</span>
                    </span>
                  </button>
                  <button
                    type="button"
                    className="session-delete"
                    title="Delete session"
                    onClick={() => void handleDeleteSession(session.id)}
                  >
                    <Trash2 size={13} aria-hidden="true" />
                  </button>
                </div>
              ))}
              {!loading && visibleSessions.length === 0 && (
                <div className="panel-empty">No sessions</div>
              )}
            </div>
          </section>
        </aside>

        <main className="chat-column">
          <section className="chat-header" aria-label="Chat context">
            <div>
              <p className="eyebrow">Live Agent</p>
              <h2>{activePersona ? activePersona.name || activePersona.id : 'Chat'}</h2>
            </div>
            <div className="chat-header-tools">
              <button
                className={`icon-button ${showSessions ? 'is-active' : ''}`}
                type="button"
                onClick={() => setShowSessions((value) => !value)}
                title={showSessions ? 'Hide sessions' : 'Show sessions'}
                aria-pressed={showSessions}
              >
                <PanelLeft size={16} aria-hidden="true" />
              </button>
              <button
                className={`icon-button ${showPanel ? 'is-active' : ''}`}
                type="button"
                onClick={() => setShowPanel((value) => !value)}
                title={showPanel ? 'Hide panel' : 'Show panel'}
                aria-pressed={showPanel}
              >
                <PanelRight size={16} aria-hidden="true" />
              </button>
              <div className="chat-badges">
                <span>
                  <Brain size={14} aria-hidden="true" />
                  {data.status?.runtime.mode ?? 'server_agent'}
                </span>
                <span>
                  <SquareTerminal size={14} aria-hidden="true" />
                  {data.status?.runtime.tool_execution ?? 'server'}
                </span>
              </div>
            </div>
          </section>

          {error && (
            <div className="error-banner" role="alert">
              {error}
            </div>
          )}

          <section className="transcript" aria-label="Live transcript">
            {loading || sessionLoading ? (
              <div className="panel-empty">
                <Loader2 className="spin" size={18} aria-hidden="true" />
                {sessionLoading ? 'Loading session' : 'Loading runtime'}
              </div>
            ) : transcriptMessages.length === 0 ? (
              <div className="empty-transcript">
                <Bot size={34} aria-hidden="true" />
                <h3>Ready</h3>
                <p>{data.status?.model ?? 'Hakimi Agent'}</p>
              </div>
            ) : (
              transcriptMessages.map((message) => (
                <article className={`message-row message-${message.role}`} key={message.id}>
                  <div className="message-avatar" aria-hidden="true">
                    {message.role === 'assistant' ? <Bot size={17} /> : <MessageSquare size={17} />}
                  </div>
                  <div className="message-body">
                    <header>
                      <span>{message.role}</span>
                      <time>{message.createdAt.toLocaleTimeString()}</time>
                    </header>
                    {message.content ? (
                      <MessageContent content={message.content} />
                    ) : sending ? (
                      <span className="message-pending">
                        <Loader2 className="spin" size={16} aria-hidden="true" />
                        Running turn
                      </span>
                    ) : null}
                    {message.content && (
                      <div className="message-actions">
                        <button
                          type="button"
                          className="message-action"
                          title="Copy"
                          onClick={() => copyMessage(message.content)}
                        >
                          <Copy size={13} aria-hidden="true" />
                        </button>
                        {!message.id.startsWith('session-') && (
                          <>
                            <button
                              type="button"
                              className="message-action"
                              title="Retry"
                              disabled={sending}
                              onClick={() => retryMessage(message)}
                            >
                              <RotateCcw size={13} aria-hidden="true" />
                            </button>
                            <button
                              type="button"
                              className="message-action"
                              title="Delete"
                              onClick={() => deleteMessage(message.id)}
                            >
                              <Trash2 size={13} aria-hidden="true" />
                            </button>
                          </>
                        )}
                      </div>
                    )}
                    {message.sessionId && <footer>session {message.sessionId}</footer>}
                  </div>
                </article>
              ))
            )}
          </section>

          <form className="composer" onSubmit={sendMessage}>
            <textarea
              value={composer}
              onChange={(event) => setComposer(event.target.value)}
              placeholder="Send a task to Hakimi"
              rows={3}
            />
            <div className="composer-footer">
              <span>
                <Activity size={14} aria-hidden="true" />
                {data.capabilities?.features.chat ? 'chat enabled' : 'chat pending'}
              </span>
              <button className="button button-primary" type="submit" disabled={sending || !composer.trim()}>
                <Send size={16} aria-hidden="true" />
                <span>Send</span>
              </button>
            </div>
          </form>
        </main>

        <aside className="right-rail">
          <nav className="panel-tabs" aria-label="Right panel">
            <button
              className={rightPanel === 'runtime' ? 'is-active' : ''}
              type="button"
              onClick={() => setRightPanel('runtime')}
              title="Runtime"
            >
              <Gauge size={17} aria-hidden="true" />
              <span>Runtime</span>
            </button>
            <button
              className={rightPanel === 'tools' ? 'is-active' : ''}
              type="button"
              onClick={() => setRightPanel('tools')}
              title="Tools"
            >
              <Wrench size={17} aria-hidden="true" />
              <span>Tools</span>
            </button>
            <button
              className={rightPanel === 'skills' ? 'is-active' : ''}
              type="button"
              onClick={() => setRightPanel('skills')}
              title="Skills"
            >
              <Layers3 size={17} aria-hidden="true" />
              <span>Skills</span>
            </button>
          </nav>

          <div className="right-panel-scroll">
            {rightPanel === 'runtime' && (
              <div className="panel-stack">
                <section className="runtime-card">
                  <header>
                    <Server size={18} aria-hidden="true" />
                    <h3>Server</h3>
                  </header>
                  <dl className="kv-grid">
                    <div>
                      <dt>Status</dt>
                      <dd>{data.status?.status ?? data.health?.status ?? 'unknown'}</dd>
                    </div>
                    <div>
                      <dt>Model</dt>
                      <dd>{data.status?.model ?? 'unknown'}</dd>
                    </div>
                    <div>
                      <dt>Auth</dt>
                      <dd>{data.status?.auth.required ? 'required' : 'open'}</dd>
                    </div>
                    <div>
                      <dt>Persistence</dt>
                      <dd>{data.status?.dashboard_admin.persistence ?? 'runtime'}</dd>
                    </div>
                  </dl>
                </section>

                <section className="runtime-card">
                  <header>
                    <Boxes size={18} aria-hidden="true" />
                    <h3>Resources</h3>
                  </header>
                  <div className="resource-grid">
                    <span>
                      <strong>{data.status?.resources.tools ?? data.tools.length}</strong>
                      tools
                    </span>
                    <span>
                      <strong>{data.mcp?.count ?? data.status?.resources.mcp_servers ?? 0}</strong>
                      MCP
                    </span>
                    <span>
                      <strong>{data.credentials?.count ?? data.status?.resources.credential_providers ?? 0}</strong>
                      credentials
                    </span>
                    <span>
                      <strong>{data.webhooks?.enabled ? 'on' : 'off'}</strong>
                      webhook
                    </span>
                  </div>
                </section>

                <section className="runtime-card">
                  <header>
                    <BadgeCheck size={18} aria-hidden="true" />
                    <h3>Capabilities</h3>
                  </header>
                  <div className="feature-list">
                    {topFeatures.map(([name, value]) => (
                      <span key={name}>
                        <i className={value ? 'feature-on' : 'feature-off'} />
                        {name.replaceAll('_', ' ')}
                        <b>{featureValue(value)}</b>
                      </span>
                    ))}
                  </div>
                </section>

                <section className="runtime-card">
                  <header>
                    <FileSearch size={18} aria-hidden="true" />
                    <h3>Session Inspector</h3>
                  </header>
                  {selectedSession ? (
                    <>
                      <div className="session-inspector-head">
                        <strong>{sessionLabel(selectedSession)}</strong>
                        <span>{selectedSession.id}</span>
                      </div>
                      <div className="message-preview-list">
                        {sessionLoading ? (
                          <div className="panel-empty">Loading messages</div>
                        ) : (
                          sessionMessages.slice(-8).map((message, index) => (
                            <article className="message-preview" key={`${message.timestamp ?? index}-${message.role}`}>
                              <header>
                                <span>{roleLabel(message.role)}</span>
                                <time>{formatDate(message.timestamp)}</time>
                              </header>
                              <p>{message.content ?? '[empty]'}</p>
                            </article>
                          ))
                        )}
                      </div>
                    </>
                  ) : (
                    <div className="panel-empty">No session selected</div>
                  )}
                </section>
              </div>
            )}

            {rightPanel === 'tools' && (
              <div className="panel-stack">
                <section className="runtime-card">
                  <header>
                    <Wrench size={18} aria-hidden="true" />
                    <h3>Tool Registry</h3>
                  </header>
                  <div className="search-field">
                    <Search size={15} aria-hidden="true" />
                    <input
                      value={toolQuery}
                      onChange={(event) => setToolQuery(event.target.value)}
                      placeholder="Filter tools"
                    />
                  </div>
                  <div className="tool-list">
                    {visibleTools.map((tool) => (
                      <article className="tool-row" key={tool.name}>
                        <strong>{tool.name}</strong>
                        <p>{tool.description}</p>
                      </article>
                    ))}
                  </div>
                </section>
                <section className="runtime-card">
                  <header>
                    <Workflow size={18} aria-hidden="true" />
                    <h3>Toolsets</h3>
                  </header>
                  <div className="toolset-list">
                    {data.toolsets.map((toolset) => (
                      <span key={toolset.name}>
                        <strong>{toolset.name}</strong>
                        <small>{toolset.source} / {toolset.tool_count}</small>
                      </span>
                    ))}
                  </div>
                </section>
              </div>
            )}

            {rightPanel === 'skills' && (
              <div className="panel-stack">
                <section className="runtime-card">
                  <header>
                    <Layers3 size={18} aria-hidden="true" />
                    <h3>Active Skills</h3>
                  </header>
                  <div className="skill-strip">
                    {activeSkills.length ? (
                      activeSkills.map((skill) => <span key={skill.name}>{skill.name}</span>)
                    ) : (
                      <span>none</span>
                    )}
                  </div>
                </section>
                <section className="runtime-card">
                  <header>
                    <Brain size={18} aria-hidden="true" />
                    <h3>Skill Catalog</h3>
                  </header>
                  <div className="skill-list">
                    {data.skills.map((skill) => (
                      <article className={`skill-row ${skill.active ? 'is-active' : ''}`} key={skill.name}>
                        <header>
                          <strong>{skill.name}</strong>
                          <span>{skill.provenance}</span>
                        </header>
                        <p>{skill.description}</p>
                        <footer>
                          {skill.tags.slice(0, 4).map((tag) => (
                            <span key={tag}>{tag}</span>
                          ))}
                        </footer>
                      </article>
                    ))}
                  </div>
                </section>
              </div>
            )}

          </div>
        </aside>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

export default App;
