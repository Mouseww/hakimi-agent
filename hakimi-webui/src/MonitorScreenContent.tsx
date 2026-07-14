import React from 'react';
import './monitor-screen-content.css';
import type { TodoItem } from './types/todo';
import type { ActiveToolCall } from './types/toolCall';

interface MonitorScreenContentProps {
  status: 'working' | 'busy' | 'planning' | 'away' | 'creative' | 'focused';
  taskHint?: string;
  todos?: TodoItem[];
  activeToolCall?: ActiveToolCall | null;
}

export const MonitorScreenContent: React.FC<MonitorScreenContentProps> = ({ status, taskHint, todos, activeToolCall }) => {
  // 如果有活跃的工具调用，最高优先级显示
  if (activeToolCall) {
    const elapsed = Math.floor((Date.now() - activeToolCall.started_at) / 1000);
    const toolDisplayName: Record<string, string> = {
      terminal: '🖥️ 执行命令',
      read_file: '📖 读取文件',
      write_file: '📝 写入文件',
      search_files: '🔍 搜索文件',
      patch: '🔧 修改文件',
      delegate_task: '👥 委派任务',
      todo: '📋 更新任务',
      web_search: '🌐 网络搜索',
    };

    return (
      <div className="screen-content tool-progress">
        <div className="tool-progress-header">
          <span className="tool-icon">⚙️</span>
          <span className="tool-name">{toolDisplayName[activeToolCall.tool_name] || `🛠️ ${activeToolCall.tool_name}`}</span>
        </div>
        <div className="tool-progress-body">
          <div className="progress-spinner"></div>
          <div className="tool-elapsed">{elapsed}s</div>
        </div>
      </div>
    );
  }

  // 如果有 todos，第二优先级显示任务列表
  if (todos && todos.length > 0) {
    const completed = todos.filter((t) => t.status === 'completed' || t.status === 'cancelled').length;

    return (
      <div className="screen-content todo-list">
        <div className="todo-header">
          <span className="todo-title">📋 任务追踪</span>
          <span className="todo-progress">{completed}/{todos.length}</span>
        </div>
        <div className="todo-items">
          {todos.slice(0, 5).map((todo) => {
            const icons = {
              pending: '○',
              in_progress: '◐',
              completed: '●',
              cancelled: '×',
            };
            const isDone = todo.status === 'completed' || todo.status === 'cancelled';

            return (
              <div key={todo.id} className={`todo-item ${isDone ? 'done' : todo.status}`}>
                <span className="todo-icon">{icons[todo.status]}</span>
                <span className="todo-content">{todo.content.slice(0, 30)}{todo.content.length > 30 ? '...' : ''}</span>
              </div>
            );
          })}
          {todos.length > 5 && (
            <div className="todo-item more">+ {todos.length - 5} 更多...</div>
          )}
        </div>
      </div>
    );
  }

  // 工作中 - 代码编辑器风格
  if (status === 'working') {
    return (
      <div className="screen-content code-editor">
        <div className="code-line">
          <span className="line-num">1</span>
          <span className="code-keyword">import</span> <span className="code-var">React</span> <span className="code-keyword">from</span> <span className="code-string">'react'</span>;
        </div>
        <div className="code-line">
          <span className="line-num">2</span>
        </div>
        <div className="code-line">
          <span className="line-num">3</span>
          <span className="code-keyword">function</span> <span className="code-func">App</span>() {'{'}
        </div>
        <div className="code-line">
          <span className="line-num">4</span>
          <span className="code-indent">  </span><span className="code-keyword">return</span> (
        </div>
        <div className="code-line">
          <span className="line-num">5</span>
          <span className="code-indent">    </span>&lt;<span className="code-tag">div</span>&gt;...&lt;/<span className="code-tag">div</span>&gt;
        </div>
        <div className="code-line">
          <span className="line-num">6</span>
          <span className="code-indent">  </span>);
        </div>
        <div className="code-line">
          <span className="line-num">7</span>
          <span className="cursor-blink">|</span>
        </div>
      </div>
    );
  }

  // 高负载 - 多窗口/终端
  if (status === 'busy') {
    return (
      <div className="screen-content terminal-output">
        <div className="terminal-line">
          <span className="terminal-prompt">$</span> <span className="terminal-cmd">npm run build</span>
        </div>
        <div className="terminal-line terminal-output-text">
          ✓ Compiled successfully
        </div>
        <div className="terminal-line terminal-output-text">
          ✓ 127 modules transformed
        </div>
        <div className="terminal-line">
          <span className="terminal-prompt">$</span> <span className="terminal-cmd">cargo test</span>
        </div>
        <div className="terminal-line terminal-output-text">
          running 42 tests...
        </div>
        <div className="terminal-line">
          <span className="cursor-blink">▊</span>
        </div>
      </div>
    );
  }

  // 项目规划 - 看板风格
  if (status === 'planning') {
    return (
      <div className="screen-content kanban-board">
        <div className="kanban-column">
          <div className="kanban-card yellow">
            <div className="card-title">Task #1</div>
          </div>
          <div className="kanban-card pink">
            <div className="card-title">Task #2</div>
          </div>
        </div>
        <div className="kanban-column">
          <div className="kanban-card blue">
            <div className="card-title">In Progress</div>
          </div>
        </div>
        <div className="kanban-column">
          <div className="kanban-card green">
            <div className="card-title">✓ Done</div>
          </div>
        </div>
      </div>
    );
  }

  // 离线/休息 - 黑屏 + 飘动的 zzz
  if (status === 'away') {
    return (
      <div className="screen-content sleep-mode">
        <div className="zzz-container">
          <span className="zzz z1">z</span>
          <span className="zzz z2">z</span>
          <span className="zzz z3">z</span>
        </div>
      </div>
    );
  }

  // 创意设计 - 图库/色板风格
  if (status === 'creative') {
    return (
      <div className="screen-content image-gallery">
        <div className="gallery-row">
          <div className="gallery-item purple"></div>
          <div className="gallery-item blue"></div>
        </div>
        <div className="gallery-row">
          <div className="gallery-item orange"></div>
          <div className="gallery-item green"></div>
        </div>
      </div>
    );
  }

  // 深度专注 - 单一窗口
  if (status === 'focused') {
    return (
      <div className="screen-content focused-view">
        <div className="focused-window">
          <div className="window-header"></div>
          <div className="window-content">
            <div className="content-line"></div>
            <div className="content-line short"></div>
            <div className="content-line"></div>
          </div>
        </div>
      </div>
    );
  }

  // 默认：显示 taskHint
  return (
    <div className="screen-content default-view">
      {taskHint && <div className="task-text">{taskHint}</div>}
    </div>
  );
};
