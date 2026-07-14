export interface ToolCallEvent {
  persona_id: string;
  tool_name: string;
  call_id: string;
  result?: string | null;
}

export interface ActiveToolCall {
  tool_name: string;
  call_id: string;
  started_at: number;
}
