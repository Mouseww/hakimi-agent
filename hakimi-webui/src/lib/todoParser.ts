import type { TodoItem } from '../types/todo';

const VALID_STATUSES = ['pending', 'in_progress', 'completed', 'cancelled'] as const;

function isRecord(v: unknown): v is Record<string, unknown> {
  return Boolean(v && typeof v === 'object' && !Array.isArray(v));
}

function isValidStatus(v: unknown): v is TodoItem['status'] {
  return typeof v === 'string' && VALID_STATUSES.includes(v as any);
}

/**
 * Parse todo items from various formats
 * Supports:
 * - Direct TodoResult object
 * - Array of TodoItems
 * - JSON string
 */
function parseTodoItems(value: unknown, depth: number): TodoItem[] | null {
  if (depth > 3) return null;

  // Array format
  if (Array.isArray(value)) {
    const items = value.flatMap((item) => {
      if (!isRecord(item) || !isValidStatus(item.status)) return [];

      const id = String(item.id ?? '').trim();
      const content = String(item.content ?? '').trim();

      return id && content
        ? [{ id, content, status: item.status as TodoItem['status'] }]
        : [];
    });

    return items.length > 0 ? items : null;
  }

  // String (JSON)
  if (typeof value === 'string' && value.trim()) {
    try {
      return parseTodoItems(JSON.parse(value), depth + 1);
    } catch {
      return null;
    }
  }

  // Object with 'todos' field
  if (isRecord(value) && 'todos' in value) {
    return parseTodoItems(value.todos, depth + 1);
  }

  return null;
}

/**
 * Parse todos from tool call result
 */
export function parseTodos(value: unknown): TodoItem[] | null {
  return parseTodoItems(value, 0);
}

/**
 * Extract todos from SSE tool call event
 */
export function todosFromToolCall(toolName: string, result: unknown, args: unknown): TodoItem[] | null {
  if (toolName !== 'todo') return null;

  // Try result first, then args
  return parseTodos(result) ?? parseTodos(args) ?? null;
}

/**
 * Check if todo list is still active (has pending or in_progress items)
 */
export function isTodoListActive(todos: TodoItem[]): boolean {
  return todos.some((t) => t.status === 'pending' || t.status === 'in_progress');
}
