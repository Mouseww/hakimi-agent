// ── Slash commands ──
const SLASH_COMMANDS = [
  { cmd: 'help', desc: '显示帮助信息' },
  { cmd: 'clear', desc: '清空当前会话消息' },
  { cmd: 'new', desc: '创建新会话' },
  { cmd: 'theme', desc: '切换主题' },
  { cmd: 'settings', desc: '打开设置' },
  { cmd: 'workspace', desc: '打开工作区' },
];

let slashCmdActive = false;

function setupSlashCommands(input) {
  // Show slash menu when typing /
  input.addEventListener('input', () => {
    const val = input.value;
    const cursorPos = input.selectionStart;
    const textBefore = val.slice(0, cursorPos);
    const slashIdx = textBefore.lastIndexOf('/');

    if (slashIdx >= 0 && !textBefore.slice(slashIdx).includes(' ')) {
      const query = textBefore.slice(slashIdx + 1).toLowerCase();
      const matches = SLASH_COMMANDS.filter(c => c.cmd.startsWith(query));
      const hint = $('slash-hint');

      if (matches.length > 0 && slashIdx === 0) {
        hint.innerHTML = matches.map(c =>
          `<span class="slash-item" data-cmd="${c.cmd}">/<strong>${c.cmd}</strong> ${c.desc}</span>`
        ).join('');
        slashCmdActive = true;
      } else {
        hint.innerHTML = '';
        slashCmdActive = false;
      }
    } else {
      $('slash-hint').innerHTML = '';
      slashCmdActive = false;
    }
  });

  // Handle slash command selection (via Enter)
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey && slashCmdActive) {
      e.preventDefault();
      const match = SLASH_COMMANDS.find(c => c.cmd.startsWith(input.value.slice(1).toLowerCase()));
      if (match) {
        input.value = '';
        $('slash-hint').innerHTML = '';
        slashCmdActive = false;
        executeSlashCommand(match.cmd);
      }
    }
  });

  // Click on slash item
  $('slash-hint').addEventListener('click', (e) => {
    const item = e.target.closest('.slash-item');
    if (item) {
      input.value = '';
      $('slash-hint').innerHTML = '';
      slashCmdActive = false;
      executeSlashCommand(item.dataset.cmd);
    }
  });
}

function executeSlashCommand(cmd) {
  switch (cmd) {
    case 'clear':
      S.messages = [];
      renderMessages();
      break;
    case 'new':
      newSession();
      break;
    case 'theme':
      toggleTheme();
      break;
    case 'settings':
      openControlCenter('settings');
      break;
    case 'workspace':
      toggleRightPanel();
      break;
    case 'help':
      const helpMsg = {
        role: 'assistant',
        content: '## 📖 Hakimi 帮助\n\n' +
          '**可用命令:**\n' +
          SLASH_COMMANDS.map(c => `- **/${c.cmd}** — ${c.desc}`).join('\n') + '\n\n' +
          '**快捷键:**\n' +
          '- `Enter` 发送消息\n' +
          '- `Shift+Enter` 换行\n' +
          '- `/` 打开命令菜单',
        id: 'help-' + Date.now(),
        timestamp: new Date().toISOString(),
      };
      S.messages.push(helpMsg);
      renderMessages();
      break;
  }
}

// ── Context ring display ──
function updateContextRing(session) {
  const ring = $('context-ring');
  if (!ring) return;

  if (!session || (!session.input_tokens && !session.output_tokens)) {
    ring.textContent = '';
    return;
  }

  const inputTokens = session.input_tokens || 0;
  const outputTokens = session.output_tokens || 0;
  const total = inputTokens + outputTokens;
  const maxCtx = session.context_length || 128000;
  const pct = Math.min(100, Math.round((total / maxCtx) * 100));

  let color = 'var(--success)';
  if (pct > 80) color = 'var(--error)';
  else if (pct > 50) color = 'var(--warning)';

  ring.innerHTML = `<span style="color:${color}">●</span> ${pct}% · ${formatTokens(total)}`;
}

function formatTokens(n) {
  if (n >= 1000) return (n / 1000).toFixed(1) + 'k';
  return n.toString();
}

// ── Update topbar model/profile badges ──
function updateTopbar() {
  const title = $('topbar-title');
  if (title && S.session) {
    title.textContent = S.session.title || S.session.name || 'Hakimi Agent';
  }
  const modelBadge = $('topbar-model');
  if (modelBadge && S.session?.model) {
    modelBadge.textContent = S.session.model;
    modelBadge.hidden = false;
  }
  updateContextRing(S.session);
}