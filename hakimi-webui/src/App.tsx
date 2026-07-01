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
  Globe,
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
import { useI18n } from './i18n';
import {
  api,
  AUTH_EVENT,
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

type DelegateStatus = {
  taskId: string;
  title: string;
  status: string;
  timestamp: string;
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

function featureValue(value: boolean | string, t: (key: 'panel.enabled' | 'panel.off') => string): string {
  if (typeof value === 'boolean') {
    return value ? t('panel.enabled') : t('panel.off');
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
  const { t, lang, setLang } = useI18n();
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
  const [view, setView] = useState<'chat' | 'config' | 'instance' | 'workspace' | 'office'>('office');
  const [editingPersona, setEditingPersona] = useState<Agent | null>(null);
  const [showSessions, setShowSessions] = useState(true);
  const [showPanel, setShowPanel] = useState(true);
  const [agentSessionList, setAgentSessionList] = useState<SessionInfo[]>([]);
  const [personaSessionMap, setPersonaSessionMap] = useState<Record<string, string | null>>({});
  const [delegateStatuses, setDelegateStatuses] = useState<DelegateStatus[]>([]);
  const [showLogin, setShowLogin] = useState(false);

  const activePersona = useMemo(
    () => agents.find((a) => a.id === activePersonaId) ?? null,
    [agents, activePersonaId],
  );
  const availableSkillNames = useMemo(() => data.skills.map((s) => s.name), [data.skills]);

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

  // Listen for 401 auth events
  useEffect(() => {
    function handleAuthRequired() {
      setShowLogin(true);
    }
    window.addEventListener(AUTH_EVENT, handleAuthRequired);
    return () => window.removeEventListener(AUTH_EVENT, handleAuthRequired);
  }, []);

  async function loadAgents() {
    try {
      const res = await api.agents();
      setAgents(res.agents);
      setActivePersonaId((current) => current ?? res.default);
    } catch {
      // agents endpoint is optional
    }
  }

  async function loadAgentSessions(agentId: string) {
    try {
      const sessions = await api.agentSessions(agentId);
      setAgentSessionList(sessions);
    } catch {
      setAgentSessionList([]);
    }
  }

  // Merge gateway sessions with agent sessions: include sessions from
  // the global pool whose source indicates gateway origin.
  const effectiveSessions = useMemo(() => {
    if (!activePersonaId) return data.sessions;
    const agentIds = new Set(agentSessionList.map((s) => s.id));
    const gatewayExtras = data.sessions.filter(
      (s) => !agentIds.has(s.id) && s.source && /gateway|telegram|slack|discord|qq/i.test(s.source),
    );
    return [...agentSessionList, ...gatewayExtras];
  }, [activePersonaId, agentSessionList, data.sessions]);

  const selectedSession = useMemo(
    () => effectiveSessions.find((session) => session.id === selectedSessionId) ?? null,
    [effectiveSessions, selectedSessionId],
  );

  const visibleSessions = useMemo(() => {
    const query = sessionQuery.trim().toLowerCase();
    if (!query) {
      return effectiveSessions;
    }
    return effectiveSessions.filter((session) => {
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
  }, [effectiveSessions, sessionQuery]);

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

    if (activePersonaId) {
      void loadAgentSessions(activePersonaId);
    }
  }

  async function loadSessionMessages(sessionId: string) {
    setSelectedSessionId(sessionId);
    setSessionLoading(true);
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
        let accumulated = '';
        response = await api.agentChatStream(activePersonaId, text, {
          sessionId: selectedSessionId ?? undefined,
          onSessionCreated: (sid) => {
            setSelectedSessionId(sid);
            if (activePersonaId) {
              void loadAgentSessions(activePersonaId);
            }
          },
          onToken: (token) => {
            if (token.startsWith('\x01')) {
              const raw = token.slice(1);
              if (raw.startsWith('hakimi_delegate:')) {
                const body = raw.slice('hakimi_delegate:'.length);
                const parts = body.split('|');
                const taskId = parts[0] ?? '';
                setDelegateStatuses((prev) => {
                  const filtered = prev.filter((d) => d.taskId !== taskId);
                  return [...filtered, { taskId, title: parts[1] ?? '', status: parts[2] ?? '', timestamp: parts[3] ?? '' }];
                });
              } else if (raw.startsWith('hakimi_tool:')) {
                setDelegateStatuses((prev) => {
                  const filtered = prev.filter((d) => d.taskId !== '__main__');
                  return [...filtered, { taskId: '__main__', title: '', status: raw.slice('hakimi_tool:'.length), timestamp: '' }];
                });
              }
              return;
            }
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
      if (activePersonaId) {
        void loadAgentSessions(activePersonaId);
      }
      void refreshAll({ quiet: true });
    } catch (sendError) {
      setMessages((current) => current.filter((message) => message.id !== assistantId));
      setError(sendError instanceof Error ? sendError.message : String(sendError));
    } finally {
      setSending(false);
      setDelegateStatuses([]);
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
    if (!window.confirm(t('sessions.deleteConfirm'))) {
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
    setShowLogin(false);
    void refreshAll({ quiet: true });
  }

  function handleLoginSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    saveAuthToken();
    void refreshAll();
    void loadAgents();
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

  useEffect(() => {
    if (activePersonaId) {
      void loadAgentSessions(activePersonaId);
    } else {
      setAgentSessionList([]);
    }
  }, [activePersonaId]);

  function handleSelectPersona(id: string) {
    if (id === activePersonaId) {
      setView('chat');
      return;
    }

    if (activePersonaId) {
      setPersonaSessionMap((prev) => ({ ...prev, [activePersonaId]: selectedSessionId }));
    }

    setActivePersonaId(id);
    setView('chat');

    const savedSessionId = personaSessionMap[id] ?? null;
    if (savedSessionId) {
      void loadSessionMessages(savedSessionId);
    } else {
      setMessages([]);
      setSessionMessages([]);
      setSelectedSessionId(null);
    }

    void loadAgentSessions(id);
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
  const sampledSessions = data.status?.resources.sessions_sampled ?? effectiveSessions.length;
  const totalTokens = effectiveSessions.reduce(
    (sum, session) => sum + session.input_tokens + session.output_tokens,
    0,
  );

  // Login screen
  if (showLogin) {
    return (
      <div className="app-shell">
        <div className="login-overlay">
          <form className="login-card" onSubmit={handleLoginSubmit}>
            <div className="brand-mark" aria-hidden="true">H</div>
            <h2>{t('auth.required')}</h2>
            <p>{t('auth.enterToken')}</p>
            <input
              type="password"
              value={authDraft}
              onChange={(event) => setAuthDraft(event.target.value)}
              placeholder={t('auth.tokenPlaceholder')}
              autoFocus
            />
            <button className="button button-primary" type="submit" disabled={!authDraft.trim()}>
              <KeyRound size={16} aria-hidden="true" />
              <span>{t('auth.login')}</span>
            </button>
          </form>
        </div>
      </div>
    );
  }

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand-lockup">
          <div className="brand-mark" aria-hidden="true">
            H
          </div>
          <div>
            <p className="eyebrow">{t('topbar.brand')}</p>
            <h1>{t('topbar.console')}</h1>
          </div>
        </div>

        <div className="topbar-status">
          <span className={`live-dot ${data.health?.status === 'ok' ? 'is-live' : ''}`} />
          <span>{data.health?.status === 'ok' ? `v${data.health.version}` : t('topbar.offline')}</span>
          <span className="topbar-divider" />
          <span>{data.status?.model ?? t('topbar.modelPending')}</span>
        </div>

        <div className="auth-cluster">
          <button
            className="icon-button"
            type="button"
            onClick={() => setLang(lang === 'zh' ? 'en' : 'zh')}
            title={t('lang.tooltip')}
          >
            <Globe size={16} aria-hidden="true" />
            <span style={{ fontSize: 11, marginLeft: 2 }}>{t('lang.switch')}</span>
          </button>
          <KeyRound size={16} aria-hidden="true" />
          <input
            aria-label={t('topbar.bearerToken')}
            type="password"
            value={authDraft}
            onChange={(event) => setAuthDraft(event.target.value)}
            placeholder={t('topbar.bearerToken')}
          />
          <button className="icon-button" type="button" onClick={saveAuthToken} title={t('topbar.saveToken')}>
            <ShieldCheck size={16} aria-hidden="true" />
          </button>
          <button
            className="icon-button"
            type="button"
            onClick={() => void refreshAll({ quiet: true })}
            disabled={refreshing}
            title={t('topbar.refresh')}
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
                <small>{t('sessions.sessions')}</small>
              </div>
              <div>
                <span>{data.tools.length}</span>
                <small>{t('sessions.tools')}</small>
              </div>
              <div>
                <span>{compactNumber(totalTokens)}</span>
                <small>{t('sessions.tokens')}</small>
              </div>
            </div>
          </section>

          <section className="rail-section">
            <div className="rail-heading">
              <div>
                <p className="eyebrow">{t('sessions.title')}</p>
                <h2>{t('sessions.recentWork')}</h2>
              </div>
              <Database size={18} aria-hidden="true" />
            </div>
            <div className="search-field">
              <Search size={15} aria-hidden="true" />
              <input
                value={sessionQuery}
                onChange={(event) => setSessionQuery(event.target.value)}
                placeholder={t('sessions.filter')}
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
                      {session.message_count} msg / {session.tool_call_count} {t('sessions.tools')}
                    </span>
                    <span className="session-foot">
                      <span>{session.model ?? t('panel.modelUnknown')}</span>
                      <span>{formatDate(session.started_at)}</span>
                    </span>
                    {session.source && /gateway|telegram|slack|discord|qq/i.test(session.source) && (
                      <span className="session-source-badge">{session.source}</span>
                    )}
                  </button>
                  <button
                    type="button"
                    className="session-delete"
                    title={t('chat.delete')}
                    onClick={() => void handleDeleteSession(session.id)}
                  >
                    <Trash2 size={13} aria-hidden="true" />
                  </button>
                </div>
              ))}
              {!loading && visibleSessions.length === 0 && (
                <div className="panel-empty">{t('sessions.none')}</div>
              )}
            </div>
          </section>
        </aside>

        <main className="chat-column">
          <section className="chat-header" aria-label="Chat context">
            <div>
              <p className="eyebrow">{t('chat.liveAgent')}</p>
              <h2>{activePersona ? activePersona.name || activePersona.id : t('chat.chat')}</h2>
            </div>
            <div className="chat-header-tools">
              <button
                className={`icon-button ${showSessions ? 'is-active' : ''}`}
                type="button"
                onClick={() => setShowSessions((value) => !value)}
                title={showSessions ? t('chat.hideSessions') : t('chat.showSessions')}
                aria-pressed={showSessions}
              >
                <PanelLeft size={16} aria-hidden="true" />
              </button>
              <button
                className={`icon-button ${showPanel ? 'is-active' : ''}`}
                type="button"
                onClick={() => setShowPanel((value) => !value)}
                title={showPanel ? t('chat.hidePanel') : t('chat.showPanel')}
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
                {sessionLoading ? t('sessions.loading') : t('sessions.loadingRuntime')}
              </div>
            ) : transcriptMessages.length === 0 ? (
              <div className="empty-transcript">
                <Bot size={34} aria-hidden="true" />
                <h3>{t('chat.ready')}</h3>
                <p>{data.status?.model ?? 'Hakimi Agent'}</p>
              </div>
            ) : (
              transcriptMessages.map((message, index) => (
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
                        {t('chat.runningTurn')}
                      </span>
                    ) : null}
                    {sending && index === transcriptMessages.length - 1 && message.role === 'assistant' && delegateStatuses.length > 0 && (
                      <div className="delegate-progress">
                        <div className="delegate-progress-header">
                          <Loader2 className="spin" size={12} aria-hidden="true" />
                          <span>{t('chat.working')}</span>
                        </div>
                        {delegateStatuses.map((d) => (
                          <div key={d.taskId} className="delegate-progress-item">
                            {d.title && <span className="delegate-title">{d.title}</span>}
                            <span className="delegate-status">{d.status}</span>
                            {d.timestamp && <span className="delegate-time">{d.timestamp}</span>}
                          </div>
                        ))}
                      </div>
                    )}
                    {message.content && (
                      <div className="message-actions">
                        <button
                          type="button"
                          className="message-action"
                          title={t('chat.copy')}
                          onClick={() => copyMessage(message.content)}
                        >
                          <Copy size={13} aria-hidden="true" />
                        </button>
                        {!message.id.startsWith('session-') && (
                          <>
                            <button
                              type="button"
                              className="message-action"
                              title={t('chat.retry')}
                              disabled={sending}
                              onClick={() => retryMessage(message)}
                            >
                              <RotateCcw size={13} aria-hidden="true" />
                            </button>
                            <button
                              type="button"
                              className="message-action"
                              title={t('chat.delete')}
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
              placeholder={t('chat.sendTask')}
              rows={3}
            />
            <div className="composer-footer">
              <span>
                <Activity size={14} aria-hidden="true" />
                {data.capabilities?.features.chat ? t('chat.chatEnabled') : t('chat.chatPending')}
              </span>
              <button className="button button-primary" type="submit" disabled={sending || !composer.trim()}>
                <Send size={16} aria-hidden="true" />
                <span>{t('chat.send')}</span>
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
              title={t('panel.runtime')}
            >
              <Gauge size={17} aria-hidden="true" />
              <span>{t('panel.runtime')}</span>
            </button>
            <button
              className={rightPanel === 'tools' ? 'is-active' : ''}
              type="button"
              onClick={() => setRightPanel('tools')}
              title={t('panel.tools')}
            >
              <Wrench size={17} aria-hidden="true" />
              <span>{t('panel.tools')}</span>
            </button>
            <button
              className={rightPanel === 'skills' ? 'is-active' : ''}
              type="button"
              onClick={() => setRightPanel('skills')}
              title={t('panel.skills')}
            >
              <Layers3 size={17} aria-hidden="true" />
              <span>{t('panel.skills')}</span>
            </button>
          </nav>

          <div className="right-panel-scroll">
            {rightPanel === 'runtime' && (
              <div className="panel-stack">
                <section className="runtime-card">
                  <header>
                    <Server size={18} aria-hidden="true" />
                    <h3>{t('panel.server')}</h3>
                  </header>
                  <dl className="kv-grid">
                    <div>
                      <dt>{t('panel.status')}</dt>
                      <dd>{data.status?.status ?? data.health?.status ?? t('panel.unknown')}</dd>
                    </div>
                    <div>
                      <dt>{t('panel.model')}</dt>
                      <dd>{data.status?.model ?? t('panel.unknown')}</dd>
                    </div>
                    <div>
                      <dt>{t('panel.auth')}</dt>
                      <dd>{data.status?.auth.required ? t('panel.required') : t('panel.open')}</dd>
                    </div>
                    <div>
                      <dt>{t('panel.persistence')}</dt>
                      <dd>{data.status?.dashboard_admin.persistence ?? 'runtime'}</dd>
                    </div>
                  </dl>
                </section>

                <section className="runtime-card">
                  <header>
                    <Boxes size={18} aria-hidden="true" />
                    <h3>{t('panel.resources')}</h3>
                  </header>
                  <div className="resource-grid">
                    <span>
                      <strong>{data.status?.resources.tools ?? data.tools.length}</strong>
                      {t('panel.tools')}
                    </span>
                    <span>
                      <strong>{data.mcp?.count ?? data.status?.resources.mcp_servers ?? 0}</strong>
                      {t('panel.mcp')}
                    </span>
                    <span>
                      <strong>{data.credentials?.count ?? data.status?.resources.credential_providers ?? 0}</strong>
                      {t('panel.credentials')}
                    </span>
                    <span>
                      <strong>{data.webhooks?.enabled ? 'on' : 'off'}</strong>
                      {t('panel.webhook')}
                    </span>
                  </div>
                </section>

                <section className="runtime-card">
                  <header>
                    <BadgeCheck size={18} aria-hidden="true" />
                    <h3>{t('panel.capabilities')}</h3>
                  </header>
                  <div className="feature-list">
                    {topFeatures.map(([name, value]) => (
                      <span key={name}>
                        <i className={value ? 'feature-on' : 'feature-off'} />
                        {name.replaceAll('_', ' ')}
                        <b>{featureValue(value, t)}</b>
                      </span>
                    ))}
                  </div>
                </section>

                <section className="runtime-card">
                  <header>
                    <FileSearch size={18} aria-hidden="true" />
                    <h3>{t('panel.sessionInspector')}</h3>
                  </header>
                  {selectedSession ? (
                    <>
                      <div className="session-inspector-head">
                        <strong>{sessionLabel(selectedSession)}</strong>
                        <span>{selectedSession.id}</span>
                      </div>
                      <div className="message-preview-list">
                        {sessionLoading ? (
                          <div className="panel-empty">{t('panel.loadingMessages')}</div>
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
                    <div className="panel-empty">{t('panel.noSession')}</div>
                  )}
                </section>
              </div>
            )}

            {rightPanel === 'tools' && (
              <div className="panel-stack">
                <section className="runtime-card">
                  <header>
                    <Wrench size={18} aria-hidden="true" />
                    <h3>{t('panel.toolRegistry')}</h3>
                  </header>
                  <div className="search-field">
                    <Search size={15} aria-hidden="true" />
                    <input
                      value={toolQuery}
                      onChange={(event) => setToolQuery(event.target.value)}
                      placeholder={t('panel.filterTools')}
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
                    <h3>{t('panel.toolsets')}</h3>
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
                    <h3>{t('panel.activeSkills')}</h3>
                  </header>
                  <div className="skill-strip">
                    {activeSkills.length ? (
                      activeSkills.map((skill) => <span key={skill.name}>{skill.name}</span>)
                    ) : (
                      <span>{t('panel.none')}</span>
                    )}
                  </div>
                </section>
                <section className="runtime-card">
                  <header>
                    <Brain size={18} aria-hidden="true" />
                    <h3>{t('panel.skillCatalog')}</h3>
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
