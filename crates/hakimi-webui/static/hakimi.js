// ═══════════════════════════════════════════════════════════════
// Hakimi WebUI — Core JS
// Vanilla JS, no framework, no build step.
// ═══════════════════════════════════════════════════════════════

'use strict';

// ── State ──
const S = {
  session: null,
  sessions: [],
  messages: [],
  busy: false,
  activeStreamId: null,
  theme: 'dark',
};

// ── DOM shortcuts ──
const $ = (id) => document.getElementById(id);
const qs = (sel, ctx) => (ctx || document).querySelector(sel);
const qsa = (sel, ctx) => (ctx || document).querySelectorAll(sel);

// ── WebUI auth token ──
const AUTH_TOKEN_KEY = 'hakimi-webui-token';

function getAuthToken() {
  try { return (localStorage.getItem(AUTH_TOKEN_KEY) || '').trim(); }
  catch (e) { return ''; }
}

function setAuthToken(token) {
  try {
    if (token) localStorage.setItem(AUTH_TOKEN_KEY, token);
    else localStorage.removeItem(AUTH_TOKEN_KEY);
  } catch (e) {}
}

function authHeaders(extra) {
  const headers = { ...(extra || {}) };
  const token = getAuthToken();
  if (token) headers.Authorization = `Bearer ${token}`;
  return headers;
}

async function promptForAuthToken(reason) {
  const token = prompt(reason || '请输入 WebUI 密码');
  if (!token) return false;
  setAuthToken(token.trim());
  return true;
}

// ── API wrapper ──
async function api(method, path, body) {
  const base = document.baseURI || location.href;
  const url = new URL(path.startsWith('/') ? path.slice(1) : path, base);
  const opts = {
    method,
    headers: authHeaders({ 'Content-Type': 'application/json' }),
    credentials: 'include',
  };
  if (body !== undefined) opts.body = JSON.stringify(body);

  const res = await fetch(url.href, opts);
  if (res.status === 401) {
    const authed = await promptForAuthToken('WebUI 需要密码，请输入后自动重试');
    if (authed) return api(method, path, body);
  }
  if (!res.ok) {
    const text = await res.text().catch(() => '');
    throw new Error(`API ${method} ${path} ${res.status}: ${text.slice(0, 200)}`);
  }
  const ct = res.headers.get('content-type') || '';
  if (ct.includes('application/json')) return res.json();
  return res.text();
}

// ── Time formatting ──
function fmtTime(ts) {
  if (!ts) return '';
  const d = new Date(ts);
  if (isNaN(d.getTime())) return '';
  return d.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' });
}

function fmtDate(ts) {
  if (!ts) return '';
  const d = new Date(ts);
  if (isNaN(d.getTime())) return '';
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  if (sameDay) return fmtTime(ts);
  return d.toLocaleDateString('zh-CN', { month: 'short', day: 'numeric' }) + ' ' + fmtTime(ts);
}

// ── Escape HTML ──
function esc(str) {
  const div = document.createElement('div');
  div.textContent = str;
  return div.innerHTML;
}

// ── Simple Markdown Renderer ──
function renderMd(text) {
  if (!text) return '';
  let html = esc(text);

  // Code blocks (before inline code) — use Prism.js if available
  html = html.replace(/```(\w*)\n?([\s\S]*?)```/g, (_, lang, code) => {
    const trimmed = esc(code.trim());
    const langClass = lang ? ` class="language-${esc(lang)}"` : '';
    let highlighted = trimmed;
    if (lang && typeof Prism !== 'undefined') {
      try {
        highlighted = Prism.highlight(code.trim(), Prism.languages[lang] || Prism.languages.plaintext, lang);
      } catch (e) { /* fallback to escaped */ }
    }
    return `<pre><code${langClass}>${highlighted}</code></pre>`;
  });

  // Inline code
  html = html.replace(/`([^`]+)`/g, '<code>$1</code>');

  // Headers
  html = html.replace(/^### (.+)$/gm, '<h3>$1</h3>');
  html = html.replace(/^## (.+)$/gm, '<h2>$1</h2>');
  html = html.replace(/^# (.+)$/gm, '<h1>$1</h1>');

  // Blockquotes
  html = html.replace(/^&gt; (.+)$/gm, '<blockquote>$1</blockquote>');

  // Bold / italic
  html = html.replace(/\*\*\*(.+?)\*\*\*/g, '<strong><em>$1</em></strong>');
  html = html.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
  html = html.replace(/\*(.+?)\*/g, '<em>$1</em>');

  // Links
  html = html.replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2" target="_blank" rel="noopener">$1</a>');

  // Lists
  html = html.replace(/^[*-] (.+)$/gm, '<li>$1</li>');
  html = html.replace(/(<li>.*<\/li>\n?)+/g, '<ul>$&</ul>');
  html = html.replace(/^\d+\. (.+)$/gm, '<li>$1</li>');

  // Paragraphs
  html = html.replace(/\n\n/g, '</p><p>');
  html = html.replace(/\n/g, '<br>');

  if (!html.startsWith('<')) html = '<p>' + html + '</p>';
  return html;
}

// ── Render one message ──
function renderMessage(msg) {
  const isUser = msg.role === 'user';
  const div = document.createElement('div');
  div.className = 'msg';
  div.dataset.msgId = msg.id || '';

  div.innerHTML = `
    <div class="msg-header">
      <div class="msg-avatar ${isUser ? 'user' : 'assistant'}">${isUser ? 'U' : 'H'}</div>
      <span class="msg-name">${isUser ? '你' : 'Hakimi'}</span>
      ${msg.tool_call_count > 0 ? `<span class="tool-badge">🛠 ${msg.tool_call_count}</span>` : ''}
      <span class="msg-time">${fmtDate(msg.timestamp || msg.created_at)}</span>
    </div>
    <div class="msg-body">${renderMd(msg.content)}</div>`;

  // Render tool calls if present (rich data from streaming)
  if (msg.tool_calls && msg.tool_calls.length > 0) {
    msg.tool_calls.forEach(tc => {
      div.appendChild(renderToolCard(tc));
    });
  }

  // Show tool call name as a simple badge if available (from history)
  if (msg.name && !isUser) {
    const badge = document.createElement('div');
    badge.className = 'tool-name-badge';
    badge.textContent = '🔧 ' + msg.name;
    div.appendChild(badge);
  }

  return div;
}

// ── Render tool call card ──
function renderToolCard(tc) {
  const card = document.createElement('div');
  card.className = 'tool-card';
  const status = tc.status || 'done';
  const statusIcon = status === 'done' ? '✅' : status === 'error' ? '❌' : '⏳';
  const args = typeof tc.args === 'object' ? JSON.stringify(tc.args, null, 2) : (tc.args || '');
  const name = tc.name || tc.tool_name || 'tool';

  card.innerHTML = `
    <div class="tool-card-header" onclick="this.nextElementSibling.hidden=!this.nextElementSibling.hidden">
      <span class="tool-status ${status}">${statusIcon}</span>
      <span style="font-family:var(--font)">${esc(name)}</span>
      <span style="margin-left:auto;font-size:10px;color:var(--text-dim)">${status}</span>
    </div>
    <div class="tool-card-body">${esc(args)}</div>`;

  if (tc.output) {
    const outDiv = document.createElement('div');
    outDiv.className = 'tool-card-body';
    outDiv.style.borderTop = '1px solid var(--border)';
    outDiv.style.color = 'var(--text-muted)';
    outDiv.textContent = tc.output;
    card.appendChild(outDiv);
  }

  return card;
}

// ── Render message list ──
function renderMessages() {
  const container = $('messages');
  container.innerHTML = '';

  if (S.messages.length === 0) {
    container.innerHTML = '<div style="text-align:center;color:var(--text-dim);padding:40px 20px;"><p>发送一条消息开始与 Hakimi 对话</p></div>';
    return;
  }

  S.messages.forEach(msg => {
    container.appendChild(renderMessage(msg));
  });

  container.scrollTop = container.scrollHeight;

  // Highlight code blocks after render
  if (typeof Prism !== 'undefined') {
    try { Prism.highlightAllUnder(container); } catch (e) {}
  }
}

// ── Append streaming chunk to last assistant message ──
function appendStreamChunk(text) {
  const container = $('messages');
  let lastMsg = container.lastElementChild;

  if (!lastMsg || lastMsg.dataset.msgId !== 'streaming') {
    const div = document.createElement('div');
    div.className = 'msg';
    div.dataset.msgId = 'streaming';
    div.innerHTML = `
      <div class="msg-header">
        <div class="msg-avatar assistant">H</div>
        <span class="msg-name">Hakimi</span>
        <span class="msg-time">${fmtTime(Date.now())}</span>
      </div>
      <div class="msg-body"></div>`;
    container.appendChild(div);
    lastMsg = div;
  }

  const body = lastMsg.querySelector('.msg-body');
  if (body) {
    const current = body.textContent;
    body.innerHTML = renderMd(current + text);
    container.scrollTop = container.scrollHeight;
  }
}

// ── Finalize streaming message ──
function finalizeStream(fullText, msgId) {
  const container = $('messages');
  const streamingMsg = container.querySelector('[data-msg-id="streaming"]');
  if (streamingMsg) {
    streamingMsg.dataset.msgId = msgId || '';
    const body = streamingMsg.querySelector('.msg-body');
    if (body) body.innerHTML = renderMd(fullText);
  }

  const existing = S.messages.find(m => m && m.id === msgId);
  if (!existing) {
    S.messages.push({
      role: 'assistant',
      content: fullText || '',
      id: msgId || 'resp-' + Date.now(),
      timestamp: new Date().toISOString(),
    });
  }
}

// ── Render sessions list ──
function renderSessions() {
  const list = $('session-list');
  list.innerHTML = '';

  if (!S.sessions || S.sessions.length === 0) {
    list.innerHTML = '<div style="padding:20px;text-align:center;color:var(--text-dim);font-size:12px;">暂无会话</div>';
    return;
  }

  S.sessions.forEach(s => {
    const item = document.createElement('div');
    const active = S.session && (S.session.id === s.id || S.session.session_id === s.id);
    item.className = 'session-item' + (active ? ' active' : '');
    item.dataset.sid = s.id;

    const title = s.title || '新会话';
    const msgCount = s.message_count || 0;
    const time = s.started_at || s.updated_at || '';

    item.innerHTML = `
      <button class="session-delete" title="删除会话" aria-label="删除会话">×</button>
      <div class="session-item-title">${esc(title)}</div>
      <div class="session-item-meta">${msgCount} 条消息${time ? ' · ' + fmtDate(time) : ''}</div>`;

    item.addEventListener('click', () => loadSession(s.id));
    const del = item.querySelector('.session-delete');
    if (del) {
      del.addEventListener('click', (e) => {
        e.preventDefault();
        e.stopPropagation();
        deleteSession(s.id, title);
      });
    }
    list.appendChild(item);
  });
}

async function deleteSession(sessionId, title) {
  if (!sessionId || S.busy) return;
  const ok = confirm(`删除会话「${title || sessionId}」？此操作不可恢复。`);
  if (!ok) return;
  try {
    await api('DELETE', `/api/sessions/${encodeURIComponent(sessionId)}`);
    S.sessions = (S.sessions || []).filter(s => s.id !== sessionId && s.session_id !== sessionId);
    if (S.session && (S.session.id === sessionId || S.session.session_id === sessionId)) {
      S.session = null;
      S.messages = [];
      renderMessages();
      updateTopbar();
    }
    renderSessions();
    await loadSessions();
  } catch (e) {
    alert('删除会话失败: ' + e.message);
  }
}

// ── Load a session ──
async function loadSession(sessionId) {
  if (S.busy) return;

  try {
    const [session, msgsResp] = await Promise.all([
      api('GET', `/api/sessions/${sessionId}`),
      api('GET', `/api/sessions/${sessionId}/messages?limit=200`),
    ]);
    S.session = session;
    S.messages = (msgsResp.messages || []).reverse();
    renderMessages();
    renderSessions();
    updateTopbar();
  } catch (e) {
    console.error('loadSession error:', e);
  }
}

// ── Load sessions list ──
async function loadSessions() {
  try {
    const resp = await api('GET', '/api/sessions?limit=50');
    S.sessions = Array.isArray(resp) ? resp : (resp.data || []);
    renderSessions();
  } catch (e) {
    console.error('loadSessions error:', e);
  }
}

// ── Create new session ──
async function newSession() {
  try {
    const resp = await api('POST', '/api/sessions', { title: '新会话' });
    const sessionObj = resp.session || resp;
    const sessionId = sessionObj.id || sessionObj.session_id;
    if (sessionId) {
      await loadSessions();
      await loadSession(sessionId);
      $('msg-input').focus();
    }
  } catch (e) {
    console.error('newSession error:', e);
  }
}

// ── Send message with SSE streaming ──
async function sendMessage() {
  const input = $('msg-input');
  const text = input.value.trim();
  if (!text || S.busy) return;

  input.value = '';
  input.style.height = 'auto';

  // Show the user's message immediately so failures are visible instead of looking like no-op.
  const userMsg = { role: 'user', content: text, id: 'local-' + Date.now(), timestamp: new Date().toISOString() };
  S.messages.push(userMsg);
  renderMessages();

  // Ensure we have a session
  if (!S.session) {
    try {
      const resp = await api('POST', '/api/sessions', { title: text.slice(0, 50) });
      const sessionObj = resp.session || resp;
      const sessionId = sessionObj.id || sessionObj.session_id;
      await loadSessions();
      S.session = { id: sessionId };
    } catch (e) {
      console.error('send: create session error:', e);
      S.messages.push({
        role: 'assistant',
        content: '❌ 无法创建会话: ' + e.message,
        id: 'err-' + Date.now(),
        timestamp: new Date().toISOString(),
      });
      renderMessages();
      return;
    }
  }

  S.busy = true;
  $('sendBtn').disabled = true;

  // ── SSE Streaming ──
  const base = document.baseURI || location.href;
  const url = new URL('api/chat/stream', base).href;
  let fullText = '';
  let streamFinished = false;

  const unlockComposer = () => {
    S.busy = false;
    const btn = $('sendBtn');
    if (btn) btn.disabled = false;
  };

  try {
    const response = await fetch(url, {
      method: 'POST',
      headers: authHeaders({ 'Content-Type': 'application/json' }),
      credentials: 'include',
      body: JSON.stringify({ message: text, session_id: S.session && (S.session.id || S.session.session_id) }),
    });

    if (response.status === 401) {
      const authed = await promptForAuthToken('WebUI 需要密码，请输入后重新发送');
      if (authed) {
        S.busy = false;
        $('sendBtn').disabled = false;
        input.value = text;
        return sendMessage();
      }
    }

    if (!response.ok) {
      throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    }

    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = '';

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });

      // Parse SSE events from buffer
      const lines = buffer.split('\n');
      buffer = lines.pop() || '';  // keep incomplete line

      let eventType = '';
      for (const line of lines) {
        if (line.startsWith('event: ')) {
          eventType = line.slice(7).trim();
        } else if (line.startsWith('data: ')) {
          const data = line.slice(6);
          if (eventType === 'token') {
            fullText += data;
            appendStreamChunk(data);
          } else if (eventType === 'done') {
            try {
              const payload = JSON.parse(data);
              fullText = payload.response || fullText;
            } catch (e) { /* use accumulated text */ }
            finalizeStream(fullText, 'resp-' + Date.now());
            streamFinished = true;
            unlockComposer();
            try { await reader.cancel(); } catch (e) {}
            break;
          } else if (eventType === 'error') {
            finalizeStream('❌ ' + data, 'err-' + Date.now());
            streamFinished = true;
            unlockComposer();
            try { await reader.cancel(); } catch (e) {}
            break;
          }
          eventType = '';
        }
      }
      if (streamFinished) break;
    }
  } catch (e) {
    console.error('sendMessage SSE error:', e);
    // Fallback: try non-streaming POST /api/chat
    try {
      const resp = await api('POST', '/api/chat', { message: text, session_id: S.session && (S.session.id || S.session.session_id) });
      const asstMsg = {
        role: 'assistant',
        content: resp.response || '',
        id: 'resp-' + Date.now(),
        timestamp: new Date().toISOString(),
      };
      S.messages.push(asstMsg);
      renderMessages();
    } catch (e2) {
      const errMsg = {
        role: 'assistant',
        content: '❌ 出错了: ' + e2.message,
        id: 'err-' + Date.now(),
        timestamp: new Date().toISOString(),
      };
      S.messages.push(errMsg);
      renderMessages();
    }
  } finally {
    unlockComposer();
    input.focus();
    loadSessions();
  }
}

// ── Health check ──
async function checkHealth() {
  try {
    const resp = await api('GET', '/api/health');
    const indicator = $('status-indicator');
    if (indicator) {
      indicator.className = 'status-dot online';
      indicator.title = '在线 · ' + (resp.version || '');
    }
    return true;
  } catch (e) {
    const indicator = $('status-indicator');
    if (indicator) {
      indicator.className = 'status-dot offline';
      indicator.title = '离线';
    }
    return false;
  }
}

// ── Theme toggle ──
function applyTheme(theme) {
  const normalized = ['dark', 'light', 'system'].includes(theme) ? theme : 'dark';
  const prefersDark = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches;
  const useDark = normalized === 'dark' || (normalized === 'system' && prefersDark);
  document.documentElement.classList.toggle('dark', useDark);
  document.documentElement.dataset.theme = normalized;
  S.theme = normalized;
  try { localStorage.setItem('hakimi-theme', normalized); } catch (e) {}
  qsa('.cc-theme-btn').forEach(btn => btn.classList.toggle('active', btn.dataset.t === normalized));
}

function setTheme(theme) {
  applyTheme(theme);
}

function toggleTheme() {
  const current = S.theme || 'dark';
  applyTheme(current === 'dark' ? 'light' : 'dark');
}

// ── Right panel toggle ──
function toggleRightPanel() {
  const panel = $('right-panel');
  if (panel) panel.hidden = !panel.hidden;
}

// ── Control Center ──
function openControlCenter(panel) {
  $('control-center').hidden = false;
  const activePanel = panel || 'settings';
  qsa('.cc-tab').forEach(t => {
    t.classList.toggle('active', t.dataset.panel === activePanel);
  });
  if (activePanel === 'settings') renderSettingsPanel();
  else if (activePanel === 'skills') renderSkillsPanel();
  else if (activePanel === 'memory') renderMemoryPanel();
  else if (activePanel === 'cron') renderCronPanel();
}

function closeControlCenter() {
  const modal = $('control-center');
  if (!modal) return;
  modal.hidden = true;
  modal.setAttribute('hidden', '');
  modal.classList.remove('open');
}

// ── CC Rendering: Settings ──
async function renderSettingsPanel() {
  const cc = $('cc-content');
  cc.innerHTML = '<div class="cc-empty">加载中…</div>';

  try {
    const [modelsResp, config] = await Promise.all([
      api('GET', '/v1/models').catch(() => ({ data: [] })),
      api('GET', '/api/config').catch(() => ({ default_model: '', theme: 'dark', password_hash: '' })),
    ]);

    const models = (modelsResp.data || modelsResp || []).filter(m => typeof m === 'string' ? m : m?.id);
    const defaultModel = config.default_model || config.model || '';
    const currentTheme = (S.theme || 'dark').toLowerCase();

    // Build model options
    let modelOptions = models.map(m => {
      const id = typeof m === 'string' ? m : m.id;
      return `<option value="${esc(id)}"${id === defaultModel ? ' selected' : ''}>${esc(id)}</option>`;
    }).join('');
    if (!modelOptions) {
      modelOptions = `<option value="" disabled>未获取到模型</option>`;
    }

    const themeOptions = ['dark', 'light', 'system'].map(t =>
      `<button class="cc-theme-btn${t === currentTheme ? ' active' : ''}" data-t="${t}" onclick="setTheme('${t}')">${t === 'dark' ? '深色' : t === 'light' ? '浅色' : '跟随系统'}</button>`
    ).join('');

    cc.innerHTML = `
      <div id="cc-settings-msg" hidden></div>
      <div class="cc-section-title">模型设置</div>
      <div class="cc-form-group">
        <label class="cc-label">默认模型</label>
        <select id="cc-model" class="cc-select">${modelOptions}</select>
      </div>
      <div class="cc-section-title">外观</div>
      <div class="cc-form-group">
        <label class="cc-label">主题</label>
        <div class="cc-theme-group">${themeOptions}</div>
      </div>
      <div class="cc-section-title">安全</div>
      <div class="cc-form-group">
        <label class="cc-label">WebUI 密码</label>
        <input id="cc-password" type="password" class="cc-input" placeholder="留空不修改密码" autocomplete="new-password">
      </div>
      <div class="cc-form-group">
        <button class="cc-btn primary" id="cc-save" onclick="saveSettings()">保存设置</button>
      </div>`;
  } catch (e) {
    cc.innerHTML = `<div class="cc-msg error">加载设置失败: ${esc(e.message)}</div>`;
  }
}

async function saveSettings() {
  const model = $('cc-model')?.value || '';
  const password = $('cc-password')?.value || '';
  try {
    const body = { default_model: model };
    if (password) body.password = password;
    // Preserve other settings
    const existing = await api('GET', '/api/config').catch(() => ({}));
    const payload = { ...(existing || {}), ...body };
    await api('POST', '/api/config', payload);
    showCCMsg('设置已保存', 'success');
  } catch (e) {
    showCCMsg('保存失败: ' + e.message, 'error');
  }
}

function showCCMsg(text, type) {
  const el = $('cc-settings-msg');
  if (!el) return;
  el.className = 'cc-msg ' + type;
  el.textContent = text;
  el.hidden = false;
  setTimeout(() => { el.hidden = true; }, 3000);
}

// ── CC Rendering: Skills ──
async function renderSkillsPanel() {
  const cc = $('cc-content');
  cc.innerHTML = '<div class="cc-empty">加载中…</div>';

  try {
    const resp = await api('GET', '/v1/skills');
    const skills = resp.skills || resp.data || resp || [];

    if (!Array.isArray(skills) || skills.length === 0) {
      cc.innerHTML = `<div class="cc-empty"><div class="cc-empty-icon">📦</div><div>暂无技能</div></div>`;
      return;
    }

    const list = skills.map(s => {
      const name = esc(s.name || s.id || '未命名');
      const desc = esc(s.description || '暂无描述');
      const cat = esc(s.category || 'general');
      const enabled = s.enabled !== false ? 'checked' : '';

      return `
        <div class="cc-skill-card">
          <div class="cc-skill-name">${name}</div>
          <div class="cc-skill-desc">${desc}</div>
          <div class="cc-skill-meta">
            <span class="cc-skill-cat">${cat}</span>
            <label class="cc-toggle" title="启用/禁用">
              <input type="checkbox" ${enabled} onchange="toggleSkill('${esc(s.name || s.id)}', this.checked)">
              <span class="cc-toggle-slider"></span>
            </label>
          </div>
        </div>
      `;
    }).join('');

    cc.innerHTML = `
      <div class="cc-section-title">技能列表</div>
      <div class="cc-skills-grid">${list}</div>
    `;
  } catch (e) {
    cc.innerHTML = `<div class="cc-msg error">加载技能失败: ${esc(e.message)}</div>`;
  }
}

function toggleSkill(name, enabled) {
  console.log('toggleSkill', name, enabled);
  // Placeholder — implement if a /v1/skills/{name} endpoint exists
}

// ── CC Rendering: Memory ──
async function renderMemoryPanel() {
  const cc = $('cc-content');
  cc.innerHTML = '<div class="cc-empty">加载中…</div>';

  try {
    const stats = await api('GET', '/api/memory/stats').catch(() => ({}));
    const entities = await api('GET', '/api/memory/entities').catch(() => ({ entities: [] }));

    const entityList = (entities.entities || entities || []).slice(0, 20);
    const entityHtml = entityList.length === 0
      ? '<div class="cc-empty"><div class="cc-empty-icon">🧠</div><div>暂无记忆</div></div>'
      : `<div class="cc-entity-list">
          ${entityList.map(e => `
            <div class="cc-entity-item">
              <span class="cc-entity-type">${esc(e.type || e.entity_type || 'entity')}</span>
              <div class="cc-entity-content">${esc(e.value || e.name || e.content || JSON.stringify(e))}</div>
            </div>
          `).join('')}
        </div>`;

    cc.innerHTML = `
      <div class="cc-section-title">记忆统计</div>
      <div class="cc-stats-grid">
        <div class="cc-stat-card">
          <div class="cc-stat-value">${stats.total_messages || stats.total || 0}</div>
          <div class="cc-stat-label">总消息数</div>
        </div>
        <div class="cc-stat-card">
          <div class="cc-stat-value">${stats.total_sessions || stats.sessions || 0}</div>
          <div class="cc-stat-label">会话数</div>
        </div>
        <div class="cc-stat-card">
          <div class="cc-stat-value">${stats.total_entities || stats.entities || 0}</div>
          <div class="cc-stat-label">实体数</div>
        </div>
        <div class="cc-stat-card">
          <div class="cc-stat-value">${stats.total_memories || stats.memories || 0}</div>
          <div class="cc-stat-label">记忆数</div>
        </div>
      </div>
      <div class="cc-section-title">搜索记忆</div>
      <div class="cc-search-row">
        <input type="search" id="cc-mem-q" class="cc-input" placeholder="搜索关键词…" onkeydown="if(event.key==='Enter')searchMemory()">
        <button class="cc-btn" onclick="searchMemory()">搜索</button>
      </div>
      <div id="cc-mem-results"></div>
      <div class="cc-section-title">最近实体</div>
      ${entityHtml}`;
  } catch (e) {
    cc.innerHTML = `<div class="cc-msg error">加载记忆失败: ${esc(e.message)}</div>`;
  }
}

async function searchMemory() {
  const input = $('cc-mem-q');
  const container = $('cc-mem-results');
  if (!input || !container) return;
  const q = input.value.trim();
  if (!q) { container.innerHTML = ''; return; }

  container.innerHTML = '<div class="cc-empty">搜索中…</div>';
  try {
    const resp = await api('GET', '/api/memory/search?q=' + encodeURIComponent(q));
    const results = resp.results || resp || [];
    if (!Array.isArray(results) || results.length === 0) {
      container.innerHTML = '<div class="cc-empty">无搜索结果</div>';
      return;
    }
    container.innerHTML = `<div class="cc-results">${results.map(r => `
      <div class="cc-result-item">
        <div class="cc-result-text">${esc(r.text || r.content || r.value || JSON.stringify(r))}</div>
        <div class="cc-result-meta">score: ${(r.score || r.relevance || 0).toFixed(3)}</div>
      </div>`).join('')}</div>`;
  } catch (e) {
    container.innerHTML = `<div class="cc-msg error">搜索失败: ${esc(e.message)}</div>`;
  }
}

// ── CC Rendering: Cron ──
async function renderCronPanel() {
  const cc = $('cc-content');
  cc.innerHTML = '<div class="cc-empty">加载中…</div>';

  try {
    const resp = await api('GET', '/api/cron/jobs').catch(() => ({ jobs: [] }));
    const rawJobs = resp.jobs || resp.data || resp || [];
    const jobs = Array.isArray(rawJobs) ? rawJobs : [];

    const table = `<table class="cc-table">
  <thead>
    <tr>
      <th>名称</th>
      <th>Schedule</th>
      <th>命令</th>
      <th>状态</th>
      <th>操作</th>
    </tr>
  </thead>
  <tbody>
    ${(jobs.length === 0 ? '<tr><td colspan="5" style="text-align:center;padding:20px;color:var(--text-dim)">暂无定时任务</td></tr>' : '')}
    ${(jobs || []).map(j => `
      <tr>
        <td>${esc(j.name || j.id || '—')}</td>
        <td><code class="cc-cron-expr">${esc(j.schedule || j.cron || '—')}</code></td>
        <td>${esc(j.command || j.task || j.action || '—')}</td>
        <td><span class="cc-skill-cat" style="${j.enabled===false ? 'background:rgba(224,80,80,0.1);color:var(--error);' : ''}">${j.enabled===false ? '已停用' : '已启用'}</span></td>
        <td>
          <button class="cc-btn cc-btn-sm danger" onclick="deleteCronJob('${esc(j.id || j.name)}')" title="删除">🗑</button>
        </td>
      </tr>
    `).join('')}
  </tbody>
</table>`;

    cc.innerHTML = `
      <div class="cc-row">
        <div class="cc-section-title" style="margin:0;border:none;flex:1">定时任务</div>
        <button class="cc-btn primary" onclick="newCronJob()">＋ 新建任务</button>
      </div>
      <div style="margin-top:12px;">${table}</div>
    `;
  } catch (e) {
    cc.innerHTML = `<div class="cc-msg error">加载定时任务失败: ${esc(e.message)}</div>`;
  }
}

function newCronJob() {
  console.log('newCronJob stub');
  alert('新建定时任务功能暂未实现');
}

function deleteCronJob(id) {
  console.log('deleteCronJob stub', id);
  alert('删除定时任务功能暂未实现 (ID: ' + id + ')');
}

// ── Textarea auto-resize ──
function autoResize(ta) {
  ta.style.height = 'auto';
  ta.style.height = Math.min(ta.scrollHeight, 200) + 'px';
}

// ═════════════════════════════════════════
// Boot
// ═════════════════════════════════════════
document.addEventListener('DOMContentLoaded', async () => {
  console.log('Hakimi WebUI booting...');
  try { applyTheme(localStorage.getItem('hakimi-theme') || S.theme || 'dark'); } catch (e) { applyTheme(S.theme || 'dark'); }

  await checkHealth();
  setInterval(checkHealth, 30000);

  await loadSessions();

  // Load most recent session
  if (S.sessions.length > 0) {
    await loadSession(S.sessions[0].id);
  }

  // ── Events ──
  $('sendBtn').addEventListener('click', sendMessage);
  
  setupSlashCommands($('msg-input'));

  $('msg-input').addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  });

  $('msg-input').addEventListener('input', (e) => autoResize(e.target));

  $('newChatBtn').addEventListener('click', newSession);
  $('toggleThemeBtn').addEventListener('click', toggleTheme);
  $('settingsBtn').addEventListener('click', () => openControlCenter('settings'));
  $('workspaceToggle').addEventListener('click', toggleRightPanel);
  $('closeRightPanel').addEventListener('click', toggleRightPanel);

  qsa('.cc-tab').forEach(tab => {
    tab.addEventListener('click', () => {
      qsa('.cc-tab').forEach(t => t.classList.remove('active'));
      tab.classList.add('active');
      const panel = tab.dataset.panel;
      if (panel === 'settings') renderSettingsPanel();
      else if (panel === 'skills') renderSkillsPanel();
      else if (panel === 'memory') renderMemoryPanel();
      else if (panel === 'cron') renderCronPanel();
    });
  });

  document.addEventListener('click', (e) => {
    const target = e.target;
    if (!(target instanceof Element)) return;
    if (target.closest('#closeCC') || target.matches('#control-center .modal-overlay')) {
      e.preventDefault();
      e.stopPropagation();
      closeControlCenter();
    }
  }, true);
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') closeControlCenter();
  });

  // Session search filter
  let searchTimer;
  $('sessionSearch').addEventListener('input', (e) => {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => {
      const q = e.target.value.toLowerCase();
      qsa('.session-item').forEach(item => {
        const title = item.querySelector('.session-item-title');
        item.style.display = title && title.textContent.toLowerCase().includes(q) ? '' : 'none';
      });
    }, 200);
  });

  // Init workspace
  if (typeof initWorkspace === 'function') {
    initWorkspace();
  }

  console.log('Hakimi WebUI ready!');
});
