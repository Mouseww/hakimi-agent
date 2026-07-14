import { useEffect } from "react";
import type { TodoItem } from "../types/todo";

interface ToolCallCompletedEvent {
  type: "tool_call_completed";
  persona_id: string;
  tool_name: string;
  call_id: string;
  result: string | null;
}

interface TodoResult {
  todos: TodoItem[];
  summary: {
    total: number;
    pending: number;
    in_progress: number;
    completed: number;
    cancelled: number;
  };
}

/**
 * Hook to subscribe to tool call events from activity SSE stream
 * and extract todo updates from `todo` tool results.
 */
export function useTodoStream(
  personaId: string,
  onTodosUpdate: (todos: TodoItem[]) => void
) {
  useEffect(() => {
    const eventSource = new EventSource("/api/activity/stream");

    eventSource.addEventListener("message", (event) => {
      try {
        const data = JSON.parse(event.data);

        // Only handle tool_call_completed events for the todo tool
        if (
          data.type === "tool_call_completed" &&
          data.persona_id === personaId &&
          data.tool_name === "todo" &&
          data.result != null
        ) {
          const evt = data as ToolCallCompletedEvent;
          if (evt.result) {
            const result: TodoResult = JSON.parse(evt.result);

            if (result.todos && Array.isArray(result.todos)) {
              onTodosUpdate(result.todos);
            }
          }
        }
      } catch (err) {
        console.error("Failed to parse activity event:", err);
      }
    });

    eventSource.addEventListener("error", (err) => {
      console.error("Activity stream connection error:", err);
      eventSource.close();
    });

    return () => {
      eventSource.close();
    };
  }, [personaId, onTodosUpdate]);
}
