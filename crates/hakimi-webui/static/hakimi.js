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

function showLoginScreen() {
  const screen = $('login-screen');
  const input = $('login-password');
  const error = $('login-error');
  if (!screen || !input) return false;
  
  screen.hidden = false;
  error.hidden = true;
  input.value = '';
  setTimeout(() => input.focus(), 100);
  
  return new Promise((resolve) => {
    const form = $('login-form');
    const handler = (e) => {
      e.preventDefault();
      const token = input.value.trim();
      if (!token) {
        error.textContent = '密码不能为空';
        error.hidden = false;
        return;
      }
      setAuthToken(token);
      screen.hidden = true;
      form.removeEventListener('submit', handler);
      resolve(true);
    };
    form.addEventListener('submit', handler);
  });
}

async function promptForAuthToken(reason) {
  return showLoginScreen();
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

  // Code blocks (must be first)
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

  // Auto-detect unformatted code blocks (tree structures, indented code)
  // Match 3+ consecutive lines with: tree chars (├─└│), pipe+dash (|——), leading spaces (4+), or comment (#)
  html = html.replace(/(?:^|\n)((?:[ ]{4,}.*|[├─└│|].*|#[^\n]*)\n){3,}/gm, (match) => {
    // Extract the block content (trim leading/trailing newlines)
    const content = match.trim();
    // Check if it's really code-like (not just a list or normal text)
    const hasTreeChars = /[├─└│]/.test(content);
    const hasPipeDash = /\|[─—]+/.test(content);
    const hasIndentation = /^[ ]{4,}/m.test(content);
    const hasComments = /^#[^\n]/m.test(content);
    
    if (hasTreeChars || hasPipeDash || (hasIndentation && hasComments)) {
      // Wrap in <pre> to preserve formatting
      return `\n<pre class="auto-detected">${content}</pre>\n`;
    }
    return match; // Not code, keep as-is
  });

  // Tables
  html = html.replace(/(\|[^\n]+\|\n)+/g, (match) => {
    const lines = match.trim().split('\n');
    if (lines.length < 2) return match;
    
    // Check if second line is a separator (|---|---|)
    if (!/^\|[\s:|-]+\|$/.test(lines[1])) return match;
    
    const headers = lines[0].split('|').slice(1, -1).map(h => h.trim());
    const rows = lines.slice(2).map(row => 
      row.split('|').slice(1, -1).map(cell => cell.trim())
    );
    
    let table = '<table><thead><tr>';
    headers.forEach(h => table += `<th>${h}</th>`);
    table += '</tr></thead><tbody>';
    rows.forEach(row => {
      table += '<tr>';
      row.forEach(cell => table += `<td>${cell}</td>`);
      table += '</tr>';
    });
    table += '</tbody></table>';
    return table;
  });

  // Horizontal rules (--- or ***)
  html = html.replace(/^(---|___|\*\*\*)$/gm, '<hr>');

  // Headers (h4 before h3, etc.)
  html = html.replace(/^#### (.+)$/gm, '<h4>$1</h4>');
  html = html.replace(/^### (.+)$/gm, '<h3>$1</h3>');
  html = html.replace(/^## (.+)$/gm, '<h2>$1</h2>');
  html = html.replace(/^# (.+)$/gm, '<h1>$1</h1>');

  // Blockquotes
  html = html.replace(/^&gt; (.+)$/gm, '<blockquote>$1</blockquote>');

  // Lists (before bold/italic to avoid conflicts with *)
  html = html.replace(/^[*-] (.+)$/gm, '<li>$1</li>');
  html = html.replace(/(<li>.*<\/li>\n?)+/g, '<ul>$&</ul>');
  html = html.replace(/^\d+\. (.+)$/gm, '<li>$1</li>');

  // Bold / italic (BEFORE converting \n to <br>, use [\s\S] to match across lines)
  html = html.replace(/\*\*\*(.+?)\*\*\*/g, '<strong><em>$1</em></strong>');
  html = html.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
  html = html.replace(/\*(.+?)\*/g, '<em>$1</em>');

  // Inline code
  html = html.replace(/`([^`]+)`/g, '<code>$1</code>');

  // Links
  html = html.replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2" target="_blank" rel="noopener">$1</a>');

  // Paragraphs and line breaks (LAST)
  // Collapse multiple newlines (3+ → 2) to avoid excessive spacing
  html = html.replace(/\n{3,}/g, '\n\n');
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
  div.dataset.role = msg.role;

  div.innerHTML = `
    <div class="msg-avatar ${isUser ? 'user' : 'assistant'}">${isUser ? 'U' : 'H'}</div>
    <div class="msg-content">
      <div class="msg-header">
        <span class="msg-name">${isUser ? '你' : 'Hakimi'}</span>
        <span class="msg-time">${fmtDate(msg.timestamp || msg.created_at)}</span>
        ${msg.tool_call_count > 0 ? `<span class="tool-badge">🛠 ${msg.tool_call_count}</span>` : ''}
      </div>
      <div class="msg-bubble">
        <div class="msg-actions">
          <button class="msg-action-btn" data-action="copy" title="复制">📋</button>
          <button class="msg-action-btn" data-action="delete" title="删除">🗑️</button>
        </div>
        <div class="msg-body">${renderMd(msg.content)}</div>
      </div>
    </div>`;

  const bubble = div.querySelector('.msg-bubble');

  // Render tool calls if present (rich data from streaming)
  if (msg.tool_calls && msg.tool_calls.length > 0) {
    msg.tool_calls.forEach(tc => {
      bubble.appendChild(renderToolCard(tc));
    });
  }

  // Show tool call name as a simple badge if available (from history)
  if (msg.name && !isUser) {
    const badge = document.createElement('div');
    badge.className = 'tool-name-badge';
    badge.textContent = '🔧 ' + msg.name;
    bubble.appendChild(badge);
  }

  // Event listeners are now handled by global delegation (see DOMContentLoaded)
  
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
    outDiv.style.whiteSpace = 'pre-wrap';
    outDiv.innerText = tc.output;  // Preserve line breaks in tool output
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
let streamingBuffer = ''; // Accumulate all chunks
let allAssistantText = ''; // Track all assistant text for final save

function appendStreamChunk(text) {
  streamingBuffer += text;
  
  // Split buffer into lines
  const lines = streamingBuffer.split('\n');
  
  // Keep the last incomplete line in buffer
  const incompleteLine = lines.pop() || '';
  
  let toolLines = [];
  let regularLines = [];
  
  // Process complete lines
  for (const line of lines) {
    if (/^[⚙️🔧🛠️💾]/.test(line) || /^hakimi_tool:/.test(line) || /^hakimi_review:/.test(line)) {
      toolLines.push(line);
    } else {
      regularLines.push(line);
    }
  }
  
  // Update tool status display
  if (toolLines.length > 0) {
    updateToolStatusDisplay(toolLines.join('\n'));
  }
  
  // Add complete lines to assistant text
  if (regularLines.length > 0) {
    for (const line of regularLines) {
      allAssistantText += line + '\n';
    }
  }
  
  // Check if incompleteLine looks like a tool call - if so, don't display it yet
  const isIncompleteTool = /^(hakimi_tool:|hakimi_review:|[⚙️🔧🛠️💾])/.test(incompleteLine);
  
  // Display everything, but hide incomplete tool lines
  const displayText = allAssistantText + (isIncompleteTool ? '' : incompleteLine);
  if (displayText.trim()) {
    displayAssistantText(displayText);
  }
  
  // Reconstruct buffer with incomplete line
  streamingBuffer = incompleteLine;
}
function displayAssistantText(text) {
  const container = $('messages');
  let lastMsg = container.lastElementChild;

  if (!lastMsg || lastMsg.dataset.msgId !== 'streaming') {
    const div = document.createElement('div');
    div.className = 'msg';
    div.dataset.msgId = 'streaming';
    div.dataset.role = 'assistant';
    div.innerHTML = `
      <div class="msg-avatar assistant">H</div>
      <div class="msg-content">
        <div class="msg-header">
          <span class="msg-name">Hakimi</span>
          <span class="msg-time">${fmtTime(Date.now())}</span>
        </div>
        <div class="msg-bubble">
          <div class="msg-actions">
            <button class="msg-action-btn" data-action="copy" title="复制">📋</button>
            <button class="msg-action-btn" data-action="delete" title="删除">🗑️</button>
          </div>
          <div class="msg-body"></div>
        </div>
      </div>`;
    container.appendChild(div);
    lastMsg = div;
    
    // Event listeners are now handled by global delegation (see DOMContentLoaded)
  }

  const body = lastMsg.querySelector('.msg-body');
  if (body) {
    // During streaming: show plain text for better performance and avoid incomplete Markdown
    // Markdown will be rendered once when streaming completes
    body.textContent = text;
    container.scrollTop = container.scrollHeight;
  }
}

// ── Display tool status in a subtle status bar ──
function updateToolStatusDisplay(statusText) {
  let statusBar = $('tool-status-bar');
  if (!statusBar) {
    statusBar = document.createElement('div');
    statusBar.id = 'tool-status-bar';
    statusBar.style.cssText = `
      position: fixed;
      bottom: 80px;
      left: 50%;
      transform: translateX(-50%);
      max-width: 600px;
      padding: 8px 16px;
      background: rgba(128, 128, 128, 0.15);
      border-radius: 8px;
      font-size: 11px;
      color: var(--text-dim);
      font-family: 'SF Mono', 'Consolas', monospace;
      white-space: pre-wrap;
      max-height: 100px;
      overflow-y: auto;
      z-index: 1000;
      opacity: 0.8;
    `;
    document.body.appendChild(statusBar);
  }
  
  // Clean up prefixes and show last few lines
  const cleanText = statusText
    .replace(/hakimi_tool:/g, '🔧')
    .replace(/hakimi_review:/g, '💭')
    .trim()
    .split('\n')
    .slice(-3)  // Only show last 3 tool calls
    .join('\n');
  
  statusBar.style.whiteSpace = 'pre-wrap';
  statusBar.innerText = cleanText;  // Preserve line breaks in status bar
}

// ── Clear tool status display ──
function clearToolStatusDisplay() {
  const statusBar = $('tool-status-bar');
  if (statusBar) {
    statusBar.remove();
  }
  streamingBuffer = '';
  allAssistantText = '';
}

// ── Finalize streaming message ──
function finalizeStream(fullText, msgId) {
  // Clear tool status display
  clearToolStatusDisplay();
  
  const container = $('messages');
  const streamingMsg = container.querySelector('[data-msg-id="streaming"]');
  
  // Use accumulated assistant text (without tool calls)
  const cleanText = allAssistantText.trim() || fullText;
  
  if (streamingMsg) {
    streamingMsg.dataset.msgId = msgId || '';
    const body = streamingMsg.querySelector('.msg-body');
    if (body) {
      // Finalize: ensure final Markdown rendering with clean text
      body.innerHTML = renderMd(cleanText);
    }
  }

  const existing = S.messages.find(m => m && m.id === msgId);
  if (!existing) {
    S.messages.push({
      role: 'assistant',
      content: cleanText || '',
      id: msgId || 'resp-' + Date.now(),
      timestamp: new Date().toISOString(),
    });
  }
  
  // Reset streaming state
  streamingBuffer = '';
  allAssistantText = '';
  
  // Clear "replying..." status
  clearReplyingStatus();
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

    item.addEventListener('click', () => { loadSession(s.id); setMobileSidebar(false); });
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

// ── Copy message content ──
function copyMessageContent(content) {
  if (!content) return;
  
  // Use Clipboard API if available
  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(content)
      .then(() => {
        showToast('✓ 已复制到剪贴板');
      })
      .catch(() => {
        // Fallback to textarea method
        fallbackCopy(content);
      });
  } else {
    fallbackCopy(content);
  }
}

function fallbackCopy(text) {
  const textarea = document.createElement('textarea');
  textarea.value = text;
  textarea.style.position = 'fixed';
  textarea.style.opacity = '0';
  document.body.appendChild(textarea);
  textarea.select();
  try {
    document.execCommand('copy');
    showToast('✓ 已复制到剪贴板');
  } catch (e) {
    showToast('✗ 复制失败');
  }
  document.body.removeChild(textarea);
}

// ── Delete message ──
async function deleteMessage(messageId, content) {
  console.log('[DEBUG] deleteMessage called, messageId:', messageId, 'type:', typeof messageId);
  
  if (!messageId) {
    console.log('[DEBUG] messageId is falsy');
    showToast('❌ 无效的消息 ID', 2000);
    return;
  }
  
  if (messageId === 'streaming') {
    console.log('[DEBUG] messageId is streaming');
    showToast('❌ 无法删除正在生成的消息', 2000);
    return;
  }
  
  if (!S.session) {
    console.log('[DEBUG] no session');
    showToast('❌ 未选择会话', 2000);
    return;
  }
  
  if (S.busy) {
    console.log('[DEBUG] system busy');
    showToast('❌ 系统忙，请稍后再试', 2000);
    return;
  }
  
  const preview = content.length > 30 ? content.substring(0, 30) + '...' : content;
  const ok = confirm(`删除消息「${preview}」？此操作不可恢复。`);
  if (!ok) {
    console.log('[DEBUG] user cancelled');
    return;
  }
  
  console.log('[DEBUG] sending DELETE request');
  try {
    await api('DELETE', `/api/sessions/${encodeURIComponent(S.session.id)}/messages/${encodeURIComponent(messageId)}`);
    
    console.log('[DEBUG] delete successful');
    // Remove from local state
    S.messages = S.messages.filter(m => m.id !== messageId);
    renderMessages();
    showToast('✓ 消息已删除');
  } catch (e) {
    console.error('[DEBUG] delete failed:', e);
    showToast('❌ 删除失败: ' + e.message, 3000);
  }
}

// ── Show toast notification ──
function showToast(message, duration = 2000) {
  // Remove existing toast
  const existing = document.querySelector('.toast');
  if (existing) existing.remove();
  
  const toast = document.createElement('div');
  toast.className = 'toast';
  toast.textContent = message;
  document.body.appendChild(toast);
  
  // Trigger animation
  setTimeout(() => toast.classList.add('show'), 10);
  
  // Auto remove
  setTimeout(() => {
    toast.classList.remove('show');
    setTimeout(() => toast.remove(), 300);
  }, duration);
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
    // Backend returns messages in chronological order (oldest → newest)
    // Keep that order: newest at the bottom (chat-style)
    S.messages = msgsResp.messages || [];
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
    const sessions = Array.isArray(resp) ? resp : (resp.data || []);
    // Reverse order: newest sessions at the bottom (chat-style)
    S.sessions = sessions.reverse();
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

  // Clear previous streaming state
  streamingBuffer = '';
  allAssistantText = '';
  clearToolStatusDisplay();
  
  // Set "replying..." status
  setReplyingStatus();

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
    clearReplyingStatus();
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

// ── Theme / skin toggle ──
const THEMES = {
  dark: { label: 'Linear Dark', desc: '冷静黑灰 + 紫色高亮' },
  obsidian: { label: 'Obsidian', desc: '深海黑 + 祖母绿高亮' },
  midnight: { label: 'Midnight', desc: '午夜紫 + 霓虹粉高亮' },
  light: { label: 'Light', desc: '通透浅色 + 蓝紫高亮' },
  system: { label: '跟随系统', desc: '自动匹配系统明暗' },
};
const THEME_ORDER = ['dark', 'obsidian', 'midnight', 'light', 'system'];

const THEME_VARS = {
  dark: {
    '--bg':'#08090a','--bg-glow':'radial-gradient(circle at 50% -20%, rgba(113,112,255,0.16), transparent 36%), radial-gradient(circle at 92% 8%, rgba(94,106,210,0.12), transparent 28%), #08090a','--sidebar-bg':'rgba(15,16,17,0.92)','--surface':'rgba(255,255,255,0.035)','--surface-2':'rgba(255,255,255,0.055)','--surface-hover':'rgba(255,255,255,0.075)','--panel':'rgba(15,16,17,0.76)','--panel-solid':'#0f1011','--border':'rgba(255,255,255,0.075)','--border-light':'rgba(255,255,255,0.14)','--text':'#f7f8f8','--text-muted':'#a8afb9','--text-dim':'#686e78','--accent':'#7170ff','--accent-hover':'#8b8aff','--accent-bg':'rgba(113,112,255,0.14)','--accent-contrast':'#ffffff','--shadow':'0 20px 70px rgba(0,0,0,0.42)','--shadow-soft':'0 10px 36px rgba(0,0,0,0.26)','--ring':'0 0 0 1px rgba(113,112,255,0.22), 0 0 0 4px rgba(113,112,255,0.12)'
  },
  obsidian: {
    '--bg':'#05070b','--bg-glow':'radial-gradient(circle at 30% -12%, rgba(24,211,160,0.18), transparent 32%), radial-gradient(circle at 88% 12%, rgba(73,131,255,0.12), transparent 28%), #05070b','--sidebar-bg':'rgba(8,12,18,0.93)','--surface':'rgba(255,255,255,0.035)','--surface-2':'rgba(255,255,255,0.06)','--surface-hover':'rgba(24,211,160,0.10)','--panel':'rgba(10,14,21,0.78)','--panel-solid':'#0a0e15','--border':'rgba(185,219,255,0.08)','--border-light':'rgba(185,219,255,0.16)','--text':'#edf7ff','--text-muted':'#a5b3c5','--text-dim':'#667286','--accent':'#18d3a0','--accent-hover':'#52e6bd','--accent-bg':'rgba(24,211,160,0.13)','--accent-contrast':'#02130e','--shadow':'0 20px 70px rgba(0,0,0,0.48)','--shadow-soft':'0 10px 36px rgba(0,0,0,0.30)','--ring':'0 0 0 1px rgba(24,211,160,0.28), 0 0 0 4px rgba(24,211,160,0.12)'
  },
  midnight: {
    '--bg':'#0a0613','--bg-glow':'radial-gradient(circle at 18% 0%, rgba(255,82,161,0.16), transparent 32%), radial-gradient(circle at 84% 8%, rgba(124,92,255,0.20), transparent 30%), #0a0613','--sidebar-bg':'rgba(16,10,29,0.92)','--surface':'rgba(255,255,255,0.04)','--surface-2':'rgba(255,255,255,0.065)','--surface-hover':'rgba(255,82,161,0.10)','--panel':'rgba(17,11,31,0.78)','--panel-solid':'#110b1f','--border':'rgba(236,207,255,0.085)','--border-light':'rgba(236,207,255,0.17)','--text':'#fbf6ff','--text-muted':'#bcaed0','--text-dim':'#776986','--accent':'#ff52a1','--accent-hover':'#ff7ab8','--accent-bg':'rgba(255,82,161,0.13)','--accent-contrast':'#ffffff','--shadow':'0 20px 70px rgba(0,0,0,0.48)','--shadow-soft':'0 10px 36px rgba(0,0,0,0.30)','--ring':'0 0 0 1px rgba(255,82,161,0.28), 0 0 0 4px rgba(255,82,161,0.12)'
  },
  light: {
    '--bg':'#f6f7fb','--bg-glow':'radial-gradient(circle at 18% -16%, rgba(94,106,210,0.16), transparent 30%), radial-gradient(circle at 90% 0%, rgba(14,165,233,0.11), transparent 28%), #f6f7fb','--sidebar-bg':'rgba(255,255,255,0.88)','--surface':'rgba(17,24,39,0.035)','--surface-2':'rgba(17,24,39,0.06)','--surface-hover':'rgba(94,106,210,0.10)','--panel':'rgba(255,255,255,0.74)','--panel-solid':'#ffffff','--border':'rgba(17,24,39,0.10)','--border-light':'rgba(17,24,39,0.18)','--text':'#111827','--text-muted':'#4b5563','--text-dim':'#8a94a6','--accent':'#5e6ad2','--accent-hover':'#4854c8','--accent-bg':'rgba(94,106,210,0.12)','--accent-contrast':'#ffffff','--shadow':'0 22px 70px rgba(17,24,39,0.14)','--shadow-soft':'0 10px 30px rgba(17,24,39,0.10)','--ring':'0 0 0 1px rgba(94,106,210,0.28), 0 0 0 4px rgba(94,106,210,0.14)'
  },
};


function resolveTheme(theme) {
  const normalized = Object.prototype.hasOwnProperty.call(THEMES, theme) ? theme : 'dark';
  if (normalized !== 'system') return normalized;
  const prefersDark = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches;
  return prefersDark ? 'dark' : 'light';
}

function applyTheme(theme) {
  const normalized = Object.prototype.hasOwnProperty.call(THEMES, theme) ? theme : 'dark';
  const resolved = resolveTheme(normalized);
  const root = document.documentElement;
  const vars = THEME_VARS[resolved] || THEME_VARS.dark;
  Object.entries(vars).forEach(([key, value]) => root.style.setProperty(key, value));
  root.classList.toggle('dark', resolved !== 'light');
  root.dataset.theme = resolved;
  root.dataset.themeChoice = normalized;
  root.style.colorScheme = resolved === 'light' ? 'light' : 'dark';
  S.theme = normalized;
  try { localStorage.setItem('hakimi-theme', normalized); } catch (e) {}
  qsa('.cc-theme-btn,.cc-theme-card').forEach(btn => btn.classList.toggle('active', btn.dataset.t === normalized));
  const toggle = $('toggleThemeBtn');
  if (toggle) toggle.title = `切换皮肤 · 当前 ${THEMES[normalized].label}`;
  window.dispatchEvent(new CustomEvent('hakimi-theme-change', { detail: { theme: normalized, resolved } }));
}

function setTheme(theme) {
  applyTheme(theme);
}

function toggleTheme() {
  const current = S.theme || 'dark';
  const idx = THEME_ORDER.indexOf(current);
  applyTheme(THEME_ORDER[(idx + 1) % THEME_ORDER.length]);
}

// ── Mobile sidebar ──
function setMobileSidebar(open) {
  const sidebar = $('sidebar');
  const scrim = $('mobile-scrim');
  if (!sidebar) return;
  sidebar.classList.toggle('mobile-open', !!open);
  document.body.classList.toggle('mobile-sidebar-open', !!open);
  if (scrim) scrim.hidden = !open;
  const topbarTitle = $('topbar-title');
  if (topbarTitle) topbarTitle.setAttribute('aria-expanded', open ? 'true' : 'false');
}

function toggleMobileSidebar() {
  const sidebar = $('sidebar');
  setMobileSidebar(!(sidebar && sidebar.classList.contains('mobile-open')));
}

function currentSessionTitle() {
  if (!S.session) return 'Hakimi Agent';
  return S.session.title || S.session.name || '新会话';
}

function updateTopbar() {
  const title = $('topbar-title');
  if (title) {
    const baseTitle = currentSessionTitle();
    const replyingStatus = $('replying-status');
    if (replyingStatus) {
      // Replying is active, keep existing status
      title.innerHTML = `${esc(baseTitle)} <span id="replying-status" style="font-size:11px;color:var(--text-dim);font-weight:normal;">正在回复...</span>`;
    } else {
      title.textContent = baseTitle;
    }
    title.title = '点击切换会话列表';
  }
}

function setReplyingStatus() {
  const title = $('topbar-title');
  if (title) {
    const baseTitle = currentSessionTitle();
    title.innerHTML = `${esc(baseTitle)} <span id="replying-status" style="font-size:11px;color:var(--text-dim);font-weight:normal;">正在回复...</span>`;
  }
}

function clearReplyingStatus() {
  const title = $('topbar-title');
  if (title) {
    title.textContent = currentSessionTitle();
    title.title = '点击切换会话列表';
  }
}

function isMobileViewport() {
  return window.matchMedia && window.matchMedia('(max-width: 860px)').matches;
}

function toggleSessionsFromTitle() {
  if (isMobileViewport()) toggleMobileSidebar();
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
  else if (activePanel === 'gateway') renderGatewayPanel();
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

    const themeOptions = THEME_ORDER.map(t => {
      const meta = THEMES[t];
      return `<button type="button" class="cc-theme-card${t === currentTheme ? ' active' : ''}" data-t="${t}" onclick="setTheme('${t}')">
        <span class="cc-theme-card-title">${esc(meta.label)}</span>
        <span class="cc-theme-card-desc">${esc(meta.desc)}</span>
        <span class="cc-theme-swatch"><span></span><span></span><span></span></span>
      </button>`;
    }).join('');

    cc.innerHTML = `
      <div id="cc-settings-msg" hidden></div>
      <div class="cc-section-title">模型设置</div>
      <div class="cc-form-group">
        <label class="cc-label">默认模型</label>
        <select id="cc-model" class="cc-select">${modelOptions}</select>
      </div>
      <div class="cc-section-title">外观</div>
      <div class="cc-form-group">
        <label class="cc-label">皮肤</label>
        <div class="cc-theme-grid">${themeOptions}</div>
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
        <td>${esc(j.prompt || j.command || j.task || j.action || '—')}</td>
        <td><span class="cc-skill-cat" style="${j.enabled===false ? 'background:rgba(224,80,80,0.1);color:var(--error);' : ''}">${j.enabled===false ? '已停用' : '已启用'}</span></td>
        <td class="cc-actions">
          <button class="cc-btn cc-btn-sm" onclick="runCronJob('${esc(j.id || j.name)}')" title="立即运行">▶</button>
          <button class="cc-btn cc-btn-sm" onclick="${j.enabled===false ? 'resumeCronJob' : 'pauseCronJob'}('${esc(j.id || j.name)}')" title="${j.enabled===false ? '恢复' : '暂停'}">${j.enabled===false ? '启' : '停'}</button>
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

async function newCronJob() {
  const name = prompt('任务名称', 'WebUI 定时任务');
  if (name === null) return;
  const schedule = prompt('执行频率：例如 every 30m、every 2h、0 9 * * *', 'every 30m');
  if (!schedule) return;
  const promptText = prompt('任务提示词 / 要执行的内容');
  if (!promptText) return;
  try {
    await api('POST', '/api/cron/jobs', {
      name: name.trim() || 'WebUI 定时任务',
      schedule: schedule.trim(),
      prompt: promptText.trim(),
      deliver: 'origin',
    });
    await renderCronPanel();
  } catch (e) {
    alert('创建定时任务失败: ' + e.message);
  }
}

async function deleteCronJob(id) {
  if (!id) return;
  if (!confirm('删除定时任务 ' + id + '？')) return;
  try {
    await api('DELETE', '/api/cron/jobs/' + encodeURIComponent(id));
    await renderCronPanel();
  } catch (e) {
    alert('删除定时任务失败: ' + e.message);
  }
}

async function pauseCronJob(id) {
  try {
    await api('POST', '/api/cron/jobs/' + encodeURIComponent(id) + '/pause');
    await renderCronPanel();
  } catch (e) {
    alert('暂停定时任务失败: ' + e.message);
  }
}

async function resumeCronJob(id) {
  try {
    await api('POST', '/api/cron/jobs/' + encodeURIComponent(id) + '/resume');
    await renderCronPanel();
  } catch (e) {
    alert('恢复定时任务失败: ' + e.message);
  }
}

async function runCronJob(id) {
  try {
    await api('POST', '/api/cron/jobs/' + encodeURIComponent(id) + '/run');
    await renderCronPanel();
  } catch (e) {
    alert('立即运行定时任务失败: ' + e.message);
  }
}

// ── CC Rendering: Gateway ──
async function renderGatewayPanel() {
  const cc = $('cc-content');
  cc.innerHTML = '<div class="cc-empty">加载中…</div>';

  try {
    const status = await api('GET', '/api/gateway/status');
    const config = await api('GET', '/api/gateway/config');

    const platformsHtml = status.running && status.platforms && status.platforms.length > 0
      ? status.platforms.map(p => `
          <div class="cc-row" style="align-items:center;padding:8px 12px;background:rgba(120,200,120,0.05);border-radius:6px;margin-bottom:8px;">
            <span style="font-size:20px;margin-right:8px;">✅</span>
            <div style="flex:1;">
              <div style="font-weight:600;">${esc(p.name || 'Unknown')}</div>
              <div style="font-size:0.85em;color:var(--text-dim);">Bot 数量: ${p.bot_count || 0}</div>
            </div>
            <span class="cc-skill-cat" style="background:rgba(120,200,120,0.15);color:#60d060;">已连接</span>
          </div>
        `).join('')
      : '<div style="padding:12px;color:var(--text-dim);text-align:center;">暂无已连接平台</div>';

    cc.innerHTML = `
      <div class="cc-section-title">网关状态</div>
      <div class="cc-row" style="margin-bottom:16px;">
        <div style="flex:1;">
          <div style="font-size:0.9em;color:var(--text-dim);margin-bottom:4px;">运行状态</div>
          <div style="font-weight:600;font-size:1.1em;color:${status.running ? 'var(--success)' : 'var(--error)'};">
            ${status.running ? '🟢 运行中' : '🔴 已停止'}
          </div>
        </div>
        <div style="flex:1;">
          <div style="font-size:0.9em;color:var(--text-dim);margin-bottom:4px;">配置已加载</div>
          <div style="font-weight:600;font-size:1.1em;">
            ${status.config_loaded ? '✅ 是' : '❌ 否'}
          </div>
        </div>
      </div>

      <div class="cc-section-title">已连接平台</div>
      <div style="margin-bottom:24px;">
        ${platformsHtml}
      </div>

      <div class="cc-section-title">网关配置</div>
      <div class="cc-row" style="margin-bottom:12px;">
        <label for="gw-busy-mode" style="flex:1;font-weight:500;">忙碌模式</label>
        <select id="gw-busy-mode" class="cc-input" style="flex:1;" onchange="updateGatewayConfig()">
          <option value="queue" ${config.busy_input_mode === 'queue' ? 'selected' : ''}>队列 (queue)</option>
          <option value="interrupt" ${config.busy_input_mode === 'interrupt' ? 'selected' : ''}>中断 (interrupt)</option>
        </select>
      </div>

      <div class="cc-row" style="margin-bottom:12px;">
        <label style="flex:1;font-weight:500;">允许所有用户</label>
        <div style="flex:1;color:var(--text-dim);">${config.allow_all ? '✅ 是' : '❌ 否'}</div>
      </div>

      <div class="cc-row" style="margin-bottom:12px;">
        <label style="flex:1;font-weight:500;">过滤旁白</label>
        <div style="flex:1;color:var(--text-dim);">${config.filter_silence_narration ? '✅ 是' : '❌ 否'}</div>
      </div>

      <div class="cc-section-title" style="margin-top:24px;">操作</div>
      <div class="cc-row">
        <button class="cc-btn danger" onclick="restartGateway()" style="flex:1;">
          🔄 重启网关
        </button>
      </div>
      <div style="margin-top:8px;font-size:0.85em;color:var(--text-dim);text-align:center;">
        ⚠️ 重启网关将重新启动整个 Hakimi 服务，WebUI 会自动重连。
      </div>
    `;
  } catch (e) {
    cc.innerHTML = `<div class="cc-msg error">加载网关状态失败: ${esc(e.message)}</div>`;
  }
}

async function updateGatewayConfig() {
  const mode = $('gw-busy-mode').value;
  try {
    await api('PATCH', '/api/gateway/config', { busy_input_mode: mode });
    // Show success message briefly
    const msg = document.createElement('div');
    msg.className = 'cc-msg success';
    msg.textContent = '配置已更新';
    msg.style.cssText = 'position:fixed;top:20px;left:50%;transform:translateX(-50%);z-index:10000;';
    document.body.appendChild(msg);
    setTimeout(() => msg.remove(), 2000);
  } catch (e) {
    alert('更新配置失败: ' + e.message);
  }
}

async function restartGateway() {
  if (!confirm('确定要重启网关吗？这将重启整个 Hakimi 服务。')) return;
  try {
    await api('POST', '/api/gateway/restart');
    // Show success message
    const msg = document.createElement('div');
    msg.className = 'cc-msg success';
    msg.textContent = '正在重启…';
    msg.style.cssText = 'position:fixed;top:20px;left:50%;transform:translateX(-50%);z-index:10000;';
    document.body.appendChild(msg);
    // Reload after 3 seconds
    setTimeout(() => location.reload(), 3000);
  } catch (e) {
    alert('重启失败: ' + e.message);
  }
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

  // Global event delegation for message action buttons
  document.addEventListener('click', (e) => {
    const target = e.target;
    if (!target.classList.contains('msg-action-btn')) return;
    
    e.stopPropagation();
    const action = target.dataset.action;
    const msgDiv = target.closest('.msg');
    if (!msgDiv) return;
    
    const msgId = msgDiv.dataset.msgId;
    const body = msgDiv.querySelector('.msg-body');
    const content = body ? body.textContent : '';
    
    console.log('[DEBUG] Action button clicked:', action, 'msgId:', msgId);
    
    if (action === 'copy') {
      copyMessageContent(content);
    } else if (action === 'delete') {
      deleteMessage(msgId, content);
    }
  });

  $('msg-input').addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  });

  $('msg-input').addEventListener('input', (e) => autoResize(e.target));

  $('newChatBtn').addEventListener('click', newSession);
  $('toggleThemeBtn').addEventListener('click', toggleTheme);
  const mobileMenuBtn = $('mobileMenuBtn');
  if (mobileMenuBtn) mobileMenuBtn.addEventListener('click', toggleMobileSidebar);
  const topbarTitle = $('topbar-title');
  if (topbarTitle) topbarTitle.addEventListener('click', toggleSessionsFromTitle);
  updateTopbar();
  const mobileScrim = $('mobile-scrim');
  if (mobileScrim) mobileScrim.addEventListener('click', () => setMobileSidebar(false));
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
      else if (panel === 'gateway') renderGatewayPanel();
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

  // Check if auth is required on page load
  checkAuthOnLoad();

  console.log('Hakimi WebUI ready!');
});

async function checkAuthOnLoad() {
  const token = getAuthToken();
  if (token) return; // Already have token, assume valid
  
  // Probe with a simple API call to see if we get 401
  try {
    await fetch('/api/config', {
      method: 'GET',
      headers: authHeaders(),
      credentials: 'include',
    });
  } catch (e) {
    // Network error, let it pass
  }
}
