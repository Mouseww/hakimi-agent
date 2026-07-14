import { useMemo } from 'react';
import type { TodoItem } from '../types/todo';

interface TodoListProps {
  todos: TodoItem[];
  compact?: boolean;
}

const STATUS_ICONS = {
  pending: '○',
  in_progress: '◐',
  completed: '●',
  cancelled: '×',
};

const STATUS_COLORS = {
  pending: '#8b949e',     // gray
  in_progress: '#58a6ff', // blue
  completed: '#3fb950',   // green
  cancelled: '#f85149',   // red
};

export function TodoList({ todos, compact = false }: TodoListProps) {
  const summary = useMemo(() => {
    const pending = todos.filter((t) => t.status === 'pending').length;
    const in_progress = todos.filter((t) => t.status === 'in_progress').length;
    const completed = todos.filter((t) => t.status === 'completed').length;
    const cancelled = todos.filter((t) => t.status === 'cancelled').length;
    return { pending, in_progress, completed, cancelled, total: todos.length };
  }, [todos]);

  if (compact) {
    return (
      <div style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: '8px',
        padding: '4px 8px',
        background: 'rgba(88, 166, 255, 0.1)',
        borderRadius: '4px',
        fontSize: '13px',
        color: '#c9d1d9',
      }}>
        <span>📋</span>
        <span>
          任务 {summary.completed + summary.cancelled}/{summary.total}
        </span>
      </div>
    );
  }

  return (
    <div style={{
      background: 'rgba(13, 17, 23, 0.6)',
      border: '1px solid rgba(48, 54, 61, 0.8)',
      borderRadius: '6px',
      padding: '12px',
      marginBottom: '12px',
    }}>
      <div style={{
        display: 'flex',
        alignItems: 'center',
        gap: '8px',
        marginBottom: '10px',
        fontSize: '14px',
        fontWeight: 500,
        color: '#c9d1d9',
      }}>
        <span>📋</span>
        <span>任务列表</span>
        <span style={{ marginLeft: 'auto', fontSize: '13px', color: '#8b949e' }}>
          {summary.completed + summary.cancelled}/{summary.total} 完成
        </span>
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
        {todos.map((todo) => (
          <div
            key={todo.id}
            style={{
              display: 'flex',
              alignItems: 'flex-start',
              gap: '8px',
              padding: '6px 8px',
              background: 'rgba(48, 54, 61, 0.3)',
              borderRadius: '4px',
              fontSize: '13px',
              opacity: todo.status === 'completed' || todo.status === 'cancelled' ? 0.6 : 1,
            }}
          >
            <span style={{
              color: STATUS_COLORS[todo.status],
              fontSize: '16px',
              lineHeight: '20px',
              flexShrink: 0,
            }}>
              {STATUS_ICONS[todo.status]}
            </span>
            <span style={{
              flex: 1,
              color: '#c9d1d9',
              lineHeight: '20px',
              textDecoration:
                todo.status === 'completed' || todo.status === 'cancelled' ? 'line-through' : 'none',
            }}>
              {todo.content}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
