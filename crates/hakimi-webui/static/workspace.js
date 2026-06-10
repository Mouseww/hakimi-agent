// ── Workspace File Browser ──
'use strict';

const Workspace = {
  currentPath: '/',
  expandedPaths: new Set(),
  contextTarget: null, // the file-item element right-clicked
};

// ── Initialize ──
function initWorkspace() {
  const content = $('right-panel-content');
  if (!content) return;

  // Build tree structure
  content.innerHTML = `
    <div id="workspace-tree">
      <div class="workspace-toolbar">
        <button id="ws-refresh" class="ws-toolbar-btn" title="刷新">↻</button>
        <input id="ws-search" type="text" placeholder="搜索…" class="ws-search-input">
      </div>
      <div id="ws-tree-container"></div>
      <div id="ws-preview" hidden>
        <div id="ws-preview-header">
          <span id="ws-preview-title"></span>
          <button id="ws-preview-close" class="icon-btn" style="font-size:12px;">✕</button>
        </div>
        <pre id="ws-preview-content"></pre>
      </div>
    </div>
    <div id="ws-context-menu" class="ws-context-menu" hidden>
      <div class="ws-context-item" data-action="new-file">📄 新建文件</div>
      <div class="ws-context-item" data-action="new-folder">📁 新建文件夹</div>
      <div class="ws-context-separator"></div>
      <div class="ws-context-item" data-action="rename">✏️ 重命名</div>
      <div class="ws-context-item" data-action="delete" style="color:var(--error)">🗑️ 删除</div>
    </div>`;

  // Wire up events
  $('closeRightPanel')?.addEventListener('click', () => {
    $('right-panel').hidden = true;
  });

  $('ws-refresh')?.addEventListener('click', () => loadWorkspace('/'));
  $('ws-preview-close')?.addEventListener('click', () => {
    $('ws-preview').hidden = true;
  });

  // Global click to close context menu
  document.addEventListener('click', (e) => {
    if (!e.target.closest('.ws-context-menu')) {
      hideContextMenu();
    }
  });

  // Context menu actions
  document.addEventListener('click', async (e) => {
    const item = e.target.closest('.ws-context-item');
    if (!item) return;
    const action = item.dataset.action;
    hideContextMenu();
    await handleContextAction(action);
  });

  // Wire workspace toggle
  $('workspaceToggle')?.addEventListener('click', () => {
    const panel = $('right-panel');
    const wasHidden = panel.hidden;
    panel.hidden = !wasHidden;
    if (wasHidden) {
      loadWorkspace('/');
    }
  });

  // Start loading
  loadWorkspace('/');
}

// ── Show/Hide right panel ──
function toggleRightPanel() {
  const panel = $('right-panel');
  const wasHidden = panel.hidden;
  panel.hidden = !wasHidden;
  if (wasHidden) {
    loadWorkspace('/');
  }
}

// ── Load workspace from API ──
async function loadWorkspace(path) {
  const container = $('ws-tree-container');
  if (!container) return;

  container.innerHTML = '<div class="ws-loading">加载中…</div>';

  try {
    const data = await api('GET', `/api/workspace/list?path=${encodeURIComponent(path)}`);
    const entries = Array.isArray(data) ? data : (data.entries || data.files || data.children || []);
    Workspace.currentPath = path;
    renderTree(entries, path);
  } catch (e) {
    console.error('workspace list error:', e);
    container.innerHTML = `<div class="ws-error">加载失败: ${esc(e.message)}</div>`;
  }
}

// ── Render file tree ──
function renderTree(entries, basePath) {
  const container = $('ws-tree-container');
  if (!container) return;

  // Breadcrumb
  const parts = basePath.split('/').filter(Boolean);
  let breadHtml = '<div class="ws-breadcrumb">';
  breadHtml += `<span class="ws-breadcrumb-item" data-path="/">🏠 /</span>`;
  let accPath = '';
  parts.forEach((p, i) => {
    accPath += '/' + p;
    breadHtml += `<span class="ws-breadcrumb-sep">/</span>`;
    breadHtml += `<span class="ws-breadcrumb-item" data-path="${accPath}">${esc(p)}</span>`;
  });
  breadHtml += '</div>';

  // Sort: dirs first, then alphabetical
  const sorted = [...entries].sort((a, b) => {
    const aDir = a.type === 'dir' || a.is_dir || a.directory;
    const bDir = b.type === 'dir' || b.is_dir || b.directory;
    if (aDir && !bDir) return -1;
    if (!aDir && bDir) return 1;
    const aName = a.name || a.path || '';
    const bName = b.name || b.path || '';
    return aName.localeCompare(bName);
  });

  let html = breadHtml;
  html += '<div class="ws-tree-list">';

  sorted.forEach(entry => {
    const name = entry.name || entry.path || '';
    const isDir = entry.type === 'dir' || entry.is_dir || entry.directory;
    const fullPath = basePath === '/' ? '/' + name : basePath + '/' + name;
    const expanded = Workspace.expandedPaths.has(fullPath);
    const gitStatus = entry.git_status || entry.git || '';

    let icon = isDir ? (expanded ? '📂' : '📁') : getFileIcon(name);
    let dotClass = '';
    let dotTitle = '';

    if (gitStatus) {
      if (gitStatus === 'M' || gitStatus === 'modified') {
        dotClass = 'ws-git-modified';
        dotTitle = '已修改';
      } else if (gitStatus === 'A' || gitStatus === 'added' || gitStatus === 'new') {
        dotClass = 'ws-git-added';
        dotTitle = '新增';
      } else if (gitStatus === 'D' || gitStatus === 'deleted') {
        dotClass = 'ws-git-deleted';
        dotTitle = '已删除';
      } else if (gitStatus === '?' || gitStatus === 'untracked') {
        dotClass = 'ws-git-untracked';
        dotTitle = '未跟踪';
      }
    }

    html += `<div class="file-item ${isDir ? 'dir' : ''}" data-path="${esc(fullPath)}" data-type="${isDir ? 'dir' : 'file'}">`;
    html += `<span class="file-icon">${icon}</span>`;
    if (dotClass) {
      html += `<span class="ws-git-dot ${dotClass}" title="${dotTitle}"></span>`;
    }
    html += `<span class="file-name">${esc(name)}</span>`;
    html += '</div>';
  });

  html += '</div>';
  container.innerHTML = html;

  // ── Wire click events ──
  container.querySelectorAll('.file-item').forEach(el => {
    el.addEventListener('click', async (e) => {
      e.stopPropagation();
      const path = el.dataset.path;
      const type = el.dataset.type;
      if (type === 'dir') {
        toggleDir(path, el);
      } else {
        openFile(path);
      }
    });

    // Context menu
    el.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      e.stopPropagation();
      showContextMenu(e.clientX, e.clientY, el);
    });
  });

  // Breadcrumb click
  container.querySelectorAll('.ws-breadcrumb-item').forEach(el => {
    el.addEventListener('click', () => {
      loadWorkspace(el.dataset.path);
    });
  });
}

// ── Toggle directory expand/collapse ──
async function toggleDir(path, el) {
  if (Workspace.expandedPaths.has(path)) {
    Workspace.expandedPaths.delete(path);
    // Reload parent to collapse
    loadWorkspace(Workspace.currentPath);
  } else {
    Workspace.expandedPaths.add(path);
    // Navigate into directory
    loadWorkspace(path);
  }
}

// ── Open file for preview ──
async function openFile(path) {
  const preview = $('ws-preview');
  const title = $('ws-preview-title');
  const content = $('ws-preview-content');
  if (!preview || !title || !content) return;

  preview.hidden = false;
  title.textContent = path.split('/').pop() || path;
  content.textContent = '加载中…';

  try {
    const data = await api('GET', `/api/workspace/read?path=${encodeURIComponent(path)}`);
    const text = typeof data === 'string' ? data : (data.content || data.text || JSON.stringify(data, null, 2));
    content.textContent = text;
  } catch (e) {
    console.error('read file error:', e);
    content.textContent = `❌ 读取失败: ${e.message}`;
  }
}

// ── Context menu ──
function showContextMenu(x, y, el) {
  const menu = $('ws-context-menu');
  if (!menu) return;

  Workspace.contextTarget = el;

  // Show/hide actions based on target type
  const isDir = el.dataset.type === 'dir';
  const items = menu.querySelectorAll('.ws-context-item');
  items.forEach(item => {
    const action = item.dataset.action;
    if (action === 'new-file' || action === 'new-folder') {
      item.hidden = !isDir;
    }
  });

  menu.hidden = false;
  menu.style.left = Math.min(x, window.innerWidth - 180) + 'px';
  menu.style.top = Math.min(y, window.innerHeight - 160) + 'px';
}

function hideContextMenu() {
  const menu = $('ws-context-menu');
  if (menu) menu.hidden = true;
}

// ── Handle context menu actions ──
async function handleContextAction(action) {
  const target = Workspace.contextTarget;
  if (!target) return;

  let basePath, name;

  switch (action) {
    case 'new-file':
    case 'new-folder': {
      basePath = target.dataset.path;
      const isFile = action === 'new-file';
      const promptMsg = isFile ? '输入文件名:' : '输入文件夹名:';
      name = prompt(promptMsg, isFile ? 'new_file.txt' : 'new_folder');
      if (!name) return;

      const fullPath = basePath === '/' ? '/' + name : basePath + '/' + name;
      try {
        await api('POST', '/api/workspace/create', { path: fullPath, type: isFile ? 'file' : 'dir' });
        loadWorkspace(basePath);
      } catch (e) {
        alert('创建失败: ' + e.message);
      }
      break;
    }

    case 'rename': {
      const fullPath = target.dataset.path;
      const oldName = fullPath.split('/').pop() || '';
      const newName = prompt('新名称:', oldName);
      if (!newName || newName === oldName) return;

      const parentPath = fullPath.substring(0, fullPath.lastIndexOf('/')) || '/';
      const newPath = parentPath === '/' ? '/' + newName : parentPath + '/' + newName;
      try {
        await api('POST', '/api/workspace/rename', { path: fullPath, new_path: newPath });
        loadWorkspace(parentPath);
      } catch (e) {
        alert('重命名失败: ' + e.message);
      }
      break;
    }

    case 'delete': {
      const delPath = target.dataset.path;
      if (!confirm(`确定要删除 "${delPath}" 吗？`)) return;

      const parentPath = delPath.substring(0, delPath.lastIndexOf('/')) || '/';
      try {
        await api('POST', '/api/workspace/delete', { path: delPath });
        loadWorkspace(parentPath);
      } catch (e) {
        alert('删除失败: ' + e.message);
      }
      break;
    }
  }
}

// ── File icon helper ──
function getFileIcon(name) {
  const ext = name.includes('.') ? name.split('.').pop().toLowerCase() : '';
  const iconMap = {
    js: '🟨', jsx: '⚛️', ts: '🔷', tsx: '⚛️',
    py: '🐍', rb: '💎', go: '🔵', rs: '🦀',
    java: '☕', c: '🔧', cpp: '🔧', h: '🔧',
    html: '🌐', css: '🎨', scss: '🎨', less: '🎨',
    json: '📋', xml: '📋', yaml: '📋', yml: '📋', toml: '📋',
    md: '📝', txt: '📄', rtf: '📄',
    sh: '💻', bash: '💻', zsh: '💻',
    sql: '🗄️', db: '🗄️',
    png: '🖼️', jpg: '🖼️', jpeg: '🖼️', gif: '🖼️', svg: '🖼️', webp: '🖼️',
    pdf: '📕', doc: '📘', docx: '📘',
    zip: '📦', tar: '📦', gz: '📦', bz2: '📦', '7z': '📦',
    exe: '⚙️', dll: '⚙️', so: '⚙️',
    lock: '🔒', env: '🔑',
    gitignore: '🙈', gitkeep: '🙈',
    cfg: '⚙️', conf: '⚙️', ini: '⚙️',
    log: '📋',
  };
  return iconMap[ext] || '📄';
}

// ── Init on DOMContentLoaded (after hakimi.js has run) ──
if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', initWorkspace);
} else {
  initWorkspace();
}

// For compatibility with toggleRightPanel in hakimi.js
// We override the existing toggleRightPanel to also load workspace
const _origToggleRightPanel = window.toggleRightPanel;
window.toggleRightPanel = function() {
  const panel = $('right-panel');
  const wasHidden = panel.hidden;
  panel.hidden = !wasHidden;
  if (wasHidden) {
    loadWorkspace('/');
  }
};