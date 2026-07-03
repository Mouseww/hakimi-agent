import {
  Activity,
  Bot,
  Brain,
  Building2,
  Copy,
  FolderTree,
  Globe,
  KeyRound,
  Loader2,
  MessageSquare,
  PanelLeft,
  Plus,
  RefreshCcw,
  RotateCcw,
  Search,
  Send,
  Settings,
  SquareTerminal,
  Trash2,
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

type UiMessage = {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  sessionId?: string;
  createdAt: Date;
  toolCalls?: Array<{ 
    name: string; 
    timestamp: Date;
    result?: string;  // Tool execution result
    expanded?: boolean;  // UI state for expand/collapse
  }>;
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

function sessionLabel(session: SessionInfo): string {
  return session.title || session.id;
}

function App() {
  const { t, lang, setLang } = useI18n();
  const [data, setData] = useState<LoadState>(emptyState);
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
  const [agents, setAgents] = useState<Agent[]>([]);
  const [activePersonaId, setActivePersonaId] = useState<string | null>(null);
  const [view, setView] = useState<'chat' | 'config' | 'instance' | 'workspace' | 'office'>('office');
  const [editingPersona, setEditingPersona] = useState<Agent | null>(null);
  const [showSessions, setShowSessions] = useState(true);
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
  // the global pool whose source starts with "gateway:" (covers all platforms).
  const effectiveSessions = useMemo(() => {
    if (!activePersonaId) return data.sessions;
    const agentIds = new Set(agentSessionList.map((s) => s.id));
    const gatewayExtras = data.sessions.filter(
      (s) =>
        !agentIds.has(s.id) &&
        s.source &&
        (s.source.startsWith('gateway:') || /^gateway$/i.test(s.source)),
    );
    return [...agentSessionList, ...gatewayExtras];
  }, [activePersonaId, agentSessionList, data.sessions]);


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
            // Hakimi backend uses \x1e (Record Separator, ASCII 30) for control messages
            // Legacy \x01 (Start of Heading, ASCII 1) is also supported for compatibility
            if (token.startsWith('\x1e') || token.startsWith('\x01')) {
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
                const toolName = raw.slice('hakimi_tool:'.length);
                // Record tool call in the message
                setMessages((current) =>
                  current.map((message) =>
                    message.id === assistantId
                      ? {
                          ...message,
                          toolCalls: [
                            ...(message.toolCalls ?? []),
                            { name: toolName, timestamp: new Date(), expanded: false },
                          ],
                        }
                      : message,
                  ),
                );
                setDelegateStatuses((prev) => {
                  const filtered = prev.filter((d) => d.taskId !== '__main__');
                  return [...filtered, { taskId: '__main__', title: '', status: toolName, timestamp: '' }];
                });
              } else if (raw.startsWith('hakimi_tool_result:')) {
                const body = raw.slice('hakimi_tool_result:'.length);
                const separatorIndex = body.indexOf('|');
                if (separatorIndex !== -1) {
                  const toolName = body.slice(0, separatorIndex);
                  const result = body.slice(separatorIndex + 1);
                  // Attach result to the most recent matching tool call
                  setMessages((current) =>
                    current.map((message) =>
                      message.id === assistantId && message.toolCalls
                        ? {
                            ...message,
                            toolCalls: message.toolCalls.map((tc, idx) =>
                              idx === message.toolCalls!.length - 1 && tc.name === toolName
                                ? { ...tc, result }
                                : tc,
                            ),
                          }
                        : message,
                    ),
                  );
                }
              }
              return;
            }
            accumulated += token;
            applyContent(accumulated);
          },
        });
      } else {
        response = await api.chat(text);
        // For non-streaming mode, set the final content
        setMessages((current) =>
          current.map((message) =>
            message.id === assistantId
              ? { ...message, content: response.response, sessionId: response.session_id }
              : message,
          ),
        );
      }
      // For streaming mode, content was already set by onToken callback
      // Only update sessionId if not already set
      if (activePersonaId) {
        setMessages((current) =>
          current.map((message) =>
            message.id === assistantId && !message.sessionId
              ? { ...message, sessionId: response.session_id }
              : message,
          ),
        );
      }
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

  function toggleToolCallExpanded(messageId: string, toolCallIndex: number) {
    setMessages((current) =>
      current.map((message) =>
        message.id === messageId && message.toolCalls
          ? {
              ...message,
              toolCalls: message.toolCalls.map((tc, idx) =>
                idx === toolCallIndex ? { ...tc, expanded: !tc.expanded } : tc,
              ),
            }
          : message,
      ),
    );
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

  const sampledSessions = data.status?.resources.sessions_sampled ?? effectiveSessions.length;

  function handleNewSession() {
    setSelectedSessionId(null);
    setMessages([]);
    setSessionMessages([]);
    setView('chat');
  }

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

        <nav className="topbar-nav">
          <button
            type="button"
            className={`topbar-nav-btn ${view === 'config' && !editingPersona ? 'is-active' : ''}`}
            onClick={handleCreatePersona}
            title={t('rail.newPersona')}
          >
            <Plus size={15} aria-hidden="true" />
            <span>{t('rail.newPersona')}</span>
          </button>
          <button
            type="button"
            className={`topbar-nav-btn ${view === 'office' ? 'is-active' : ''}`}
            onClick={() => setView('office')}
            title={t('rail.office')}
          >
            <Building2 size={15} aria-hidden="true" />
            <span>{t('rail.office')}</span>
          </button>
          <button
            type="button"
            className={`topbar-nav-btn ${view === 'workspace' ? 'is-active' : ''}`}
            onClick={() => setView('workspace')}
            title={t('rail.workspace')}
          >
            <FolderTree size={15} aria-hidden="true" />
            <span>{t('rail.workspace')}</span>
          </button>
          <button
            type="button"
            className={`topbar-nav-btn ${view === 'instance' ? 'is-active' : ''}`}
            onClick={() => setView('instance')}
            title={t('rail.instance')}
          >
            <Settings size={15} aria-hidden="true" />
            <span>{t('rail.instance')}</span>
          </button>
        </nav>

        <div className="topbar-actions">
          <button
            className="icon-button"
            type="button"
            onClick={() => setLang(lang === 'zh' ? 'en' : 'zh')}
            title={t('lang.tooltip')}
          >
            <Globe size={16} aria-hidden="true" />
            <span style={{ fontSize: 11, marginLeft: 2 }}>{t('lang.switch')}</span>
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
              className={`workspace-grid ${showSessions ? '' : 'sessions-collapsed'}`}
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
            </div>
          </section>

          <section className="rail-section">
            <div className="rail-heading">
              <div>
                <p className="eyebrow">{t('sessions.title')}</p>
                <h2>{t('sessions.recentWork')}</h2>
              </div>
              <button
                type="button"
                className="icon-button"
                title={t('sessions.newSession')}
                onClick={handleNewSession}
              >
                <Plus size={16} aria-hidden="true" />
              </button>
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
                    {message.toolCalls && message.toolCalls.length > 0 && (
                      <div className="message-tool-calls">
                        {message.toolCalls.map((tc, idx) => (
                          <div key={idx} className={`tool-call-item ${tc.result ? 'has-result' : ''}`}>
                            <button
                              type="button"
                              className="tool-call-header"
                              onClick={() => tc.result && toggleToolCallExpanded(message.id, idx)}
                              disabled={!tc.result}
                            >
                              <span className="tool-call-icon">⚙️</span>
                              <span className="tool-call-name">{tc.name}</span>
                              {tc.result && (
                                <span className="tool-call-toggle">
                                  {tc.expanded ? '▼' : '▶'}
                                </span>
                              )}
                            </button>
                            {tc.result && tc.expanded && (
                              <div className="tool-call-result">
                                <MessageContent content={tc.result} />
                              </div>
                            )}
                          </div>
                        ))}
                      </div>
                    )}
                    {message.content ? (
                      <MessageContent content={message.content} />
                    ) : sending ? (
                      <span className="message-pending">
                        <Loader2 className="spin" size={16} aria-hidden="true" />
                        {t('chat.runningTurn')}
                      </span>
                    ) : null}
                    {sending && index === transcriptMessages.length - 1 && message.role === 'assistant' && delegateStatuses.length > 0 && (() => {
                      const runningCount = delegateStatuses.filter(d => !d.status?.match(/done|complete|finish|error|fail/i)).length;
                      const isParallel = delegateStatuses.length > 1;
                      return (
                        <div className="delegate-progress">
                          <div className="delegate-progress-header">
                            <Loader2 className="spin" size={12} aria-hidden="true" />
                            <span>{t('chat.working')}</span>
                            {isParallel && (
                              <span className="delegate-parallel-badge">
                                {runningCount > 0 ? `${runningCount} ${t('chat.parallel')}` : t('chat.parallel')}
                              </span>
                            )}
                          </div>
                          <div className="delegate-progress-lanes">
                            {delegateStatuses.map((d) => {
                              const isDone = !!d.status?.match(/done|complete|finish/i);
                              const isError = !!d.status?.match(/error|fail/i);
                              const dotClass = isError ? 'is-error' : isDone ? 'is-done' : 'is-running';
                              return (
                                <div key={d.taskId} className="delegate-progress-item">
                                  <span className={`delegate-lane-dot ${dotClass}`} />
                                  <div className="delegate-item-body">
                                    {d.title && <span className="delegate-title">{d.title}</span>}
                                    <span className="delegate-status">{d.status}</span>
                                    {d.timestamp && <span className="delegate-time">{d.timestamp}</span>}
                                  </div>
                                </div>
                              );
                            })}
                          </div>
                          {isParallel && runningCount > 0 && (
                            <div className="delegate-parallel-wave">
                              {Array.from({ length: 8 }, (_, i) => (
                                <span key={i} className="delegate-wave-bar" />
                              ))}
                            </div>
                          )}
                        </div>
                      );
                    })()}
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

            </div>
          )}
        </div>
      </div>
    </div>
  );
}

export default App;
