const AUTH_TOKEN_KEY = 'hakimi-webui-token';

export interface ChatRequest {
  message: string;
}

export interface ChatResponse {
  response: string;
  session_id: string;
}

export interface HealthResponse {
  status: string;
  version: string;
}

export interface DashboardStatus {
  object: string;
  status: string;
  version: string;
  model: string;
  auth: {
    type: string;
    required: boolean;
  };
  runtime: {
    mode: string;
    tool_execution: string;
  };
  resources: {
    sessions_sampled: number;
    tools: number;
    mcp_servers: number;
    credential_providers: number;
    webhook_enabled: boolean;
  };
  dashboard_admin: {
    readonly: boolean;
    write_operations: boolean;
    persistence: string;
  };
}

export interface CapabilityEndpoint {
  method: string;
  path: string;
}

export interface CapabilitiesResponse {
  object: string;
  platform: string;
  model: string;
  auth: {
    type: string;
    required: boolean;
  };
  runtime: {
    mode: string;
    tool_execution: string;
    split_runtime: boolean;
    description: string;
  };
  features: Record<string, boolean | string>;
  dashboard_admin: Record<string, boolean | string>;
  endpoints: Record<string, CapabilityEndpoint>;
}

export interface SessionInfo {
  id: string;
  source: string | null;
  user_id: string | null;
  model: string | null;
  started_at: string | null;
  ended_at: string | null;
  message_count: number;
  tool_call_count: number;
  input_tokens: number;
  output_tokens: number;
  title: string | null;
}

export interface SessionMessageInfo {
  role: string;
  content: string | null;
  timestamp: string | null;
  tool_call_id: string | null;
  name: string | null;
  tool_call_count: number;
  has_reasoning: boolean;
  token_count: number | null;
  finish_reason: string | null;
}

export interface SessionMessagesResponse {
  object: string;
  session: SessionInfo;
  count: number;
  messages: SessionMessageInfo[];
}

export interface SessionSearchResultInfo {
  session_id: string;
  message_id: number;
  content: string | null;
  rank: number;
  title: string | null;
  source: string | null;
  model: string | null;
  started_at: string | null;
}

export interface SessionSearchResponse {
  object: string;
  query: string;
  count: number;
  data: SessionSearchResultInfo[];
}

export interface ToolInfo {
  name: string;
  description: string;
  parameters: unknown;
}

export interface SkillInfo {
  name: string;
  description: string;
  trigger: string | null;
  tags: string[];
  phases: string[];
  platforms: string[];
  provenance: string;
  active: boolean;
}

export interface SkillsResponse {
  object: string;
  total: number;
  active: string[];
  data: SkillInfo[];
}

export interface ToolsetToolInfo {
  name: string;
  description: string;
  parameters: unknown;
}

export interface ToolsetInfo {
  name: string;
  source: string;
  deferrable: boolean;
  tool_count: number;
  tools: ToolsetToolInfo[];
}

export interface ToolsetsResponse {
  object: string;
  total_toolsets: number;
  total_tools: number;
  data: ToolsetInfo[];
}

export interface McpServersResponse {
  object: string;
  servers: Array<{
    name: string;
    transport: string;
    command: string;
    args_count: number;
    env_count: number;
  }>;
  count: number;
  secrets_redacted: boolean;
  write_operations: boolean;
  persistence: string;
}

export interface CredentialPoolResponse {
  object: string;
  providers: Array<{
    provider: string;
    strategy: string;
    count: number;
  }>;
  count: number;
  secrets_redacted: boolean;
  write_operations: boolean;
  persistence: string;
}

export interface WebhookResponse {
  object: string;
  enabled: boolean;
  bot_id: string;
  port: number;
  path: string;
  secret_configured: boolean;
  secrets_redacted: boolean;
  write_operations: boolean;
  persistence: string;
}

export interface SanitizedConfig {
  model_default: string;
  model_provider: string;
  agent_max_turns: number;
  agent_verbose: boolean;
  agent_system_prompt: string;
  agent_reasoning_effort: string;
  agent_save_trajectories: boolean;
  agent_trajectory_dir: string;
  terminal_env_type: string;
  terminal_cwd: string;
  terminal_timeout: number;
  terminal_docker_image: string;
  compression_enabled: boolean;
  compression_engine: string;
  compression_model: string;
  compression_context_length: number;
  display_streaming: boolean;
  display_skin: string;
  embedding_enabled: boolean;
  embedding_provider: string;
  embedding_model: string;
  embedding_dimension: number;
  embedding_batch_size: number;
  embedding_normalize: boolean;
  mcp_server_count: number;
}

export interface ConfigUpdate {
  model_default?: string;
  model_provider?: string;
  agent_max_turns?: number;
  agent_verbose?: boolean;
  agent_system_prompt?: string;
  agent_reasoning_effort?: string;
  agent_save_trajectories?: boolean;
  agent_trajectory_dir?: string;
  terminal_env_type?: string;
  terminal_cwd?: string;
  terminal_timeout?: number;
  terminal_docker_image?: string;
  compression_enabled?: boolean;
  compression_engine?: string;
  compression_model?: string;
  compression_context_length?: number;
  display_streaming?: boolean;
  display_skin?: string;
  embedding_enabled?: boolean;
  embedding_provider?: string;
  embedding_model?: string;
  embedding_dimension?: number;
  embedding_batch_size?: number;
  embedding_normalize?: boolean;
}

export interface Agent {
  id: string;
  name: string;
  avatar: string;
  description: string;
  model: string;
  reasoning_effort?: string | null;
  system_prompt: string;
  enabled_skills: string[];
  bindings: string[];
  is_default: boolean;
}

export interface AgentsListResponse {
  agents: Agent[];
  default: string;
}

export interface AgentUpdate {
  name?: string;
  avatar?: string;
  description?: string;
  model?: string;
  reasoning_effort?: string;
  system_prompt?: string;
  enabled_skills?: string[];
  bindings?: string[];
  is_default?: boolean;
}

export interface BindingsResponse {
  bindings: Record<string, string>;
  default: string;
}

export function getAuthToken(): string {
  return window.localStorage.getItem(AUTH_TOKEN_KEY) ?? '';
}

export function setAuthToken(token: string): void {
  const trimmed = token.trim();
  if (trimmed) {
    window.localStorage.setItem(AUTH_TOKEN_KEY, trimmed);
  } else {
    window.localStorage.removeItem(AUTH_TOKEN_KEY);
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const token = getAuthToken();
  const headers: Record<string, string> = {
    ...(init?.body ? { 'Content-Type': 'application/json' } : {}),
    ...(init?.headers as Record<string, string> | undefined),
  };

  if (token) {
    headers.Authorization = `Bearer ${token}`;
  }

  const response = await fetch(path, {
    ...init,
    headers,
  });

  if (!response.ok) {
    let message = `${response.status} ${response.statusText}`;
    try {
      const payload = (await response.json()) as { error?: string };
      message = payload.error ?? message;
    } catch {
      // Middleware can return an empty body for authentication failures.
    }
    throw new Error(message);
  }

  return (await response.json()) as T;
}

export interface GatewayPlatformStatus {
  name?: string;
  platform?: string;
  bot_id?: string;
  messages_sent?: number;
  connected?: boolean;
  bot_count?: number;
}

export interface GatewayStatusResponse {
  running: boolean;
  platforms: GatewayPlatformStatus[];
  total_messages_sent?: number;
  config_loaded?: boolean;
}

export interface GatewayConfigResponse {
  busy_input_mode: string;
  allow_all: boolean;
  allowed_users: string[];
  filter_silence_narration: boolean;
}

export const api = {
  health: () => request<HealthResponse>('/api/health'),
  status: () => request<DashboardStatus>('/api/status'),
  capabilities: () => request<CapabilitiesResponse>('/v1/capabilities'),
  chat: (message: string) =>
    request<ChatResponse>('/api/chat', {
      method: 'POST',
      body: JSON.stringify({ message } satisfies ChatRequest),
    }),
  sessions: () => request<SessionInfo[]>('/api/sessions'),
  sessionMessages: (sessionId: string, limit = 80) =>
    request<SessionMessagesResponse>(
      `/api/sessions/${encodeURIComponent(sessionId)}/messages?limit=${limit}`,
    ),
  searchSessions: (query: string, limit = 20) =>
    request<SessionSearchResponse>(
      `/api/sessions/search?q=${encodeURIComponent(query)}&limit=${limit}`,
    ),
  tools: () => request<ToolInfo[]>('/api/tools'),
  skills: () => request<SkillsResponse>('/v1/skills'),
  toolsets: () => request<ToolsetsResponse>('/v1/toolsets'),
  mcpServers: () => request<McpServersResponse>('/api/mcp/servers'),
  credentialPools: () => request<CredentialPoolResponse>('/api/credentials/pool'),
  webhooks: () => request<WebhookResponse>('/api/webhooks'),
  config: () => request<SanitizedConfig>('/api/config'),
  updateConfig: (payload: ConfigUpdate) =>
    request<SanitizedConfig>('/api/config', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  getGatewayStatus: () => request<GatewayStatusResponse>('/api/gateway/status'),
  getGatewayConfig: () => request<GatewayConfigResponse>('/api/gateway/config'),
  updateGatewayConfig: (config: GatewayConfigResponse) =>
    request<{ success: boolean; message: string }>('/api/gateway/config', {
      method: 'PATCH',
      body: JSON.stringify(config),
    }),
  restartGateway: () =>
    request<{ success: boolean; message: string }>('/api/gateway/restart', {
      method: 'POST',
    }),
  agents: () => request<AgentsListResponse>('/api/agents'),
  agent: (id: string) => request<Agent>(`/api/agents/${encodeURIComponent(id)}`),
  createAgent: (payload: Partial<Agent> & { id: string }) =>
    request<Agent>('/api/agents', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  updateAgent: (id: string, payload: AgentUpdate) =>
    request<Agent>(`/api/agents/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      body: JSON.stringify(payload),
    }),
  deleteAgent: (id: string) =>
    request<{ id: string; deleted: boolean }>(`/api/agents/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    }),
  agentChat: (id: string, message: string) =>
    request<ChatResponse>(`/api/agents/${encodeURIComponent(id)}/chat`, {
      method: 'POST',
      body: JSON.stringify({ message } satisfies ChatRequest),
    }),
  bindings: () => request<BindingsResponse>('/api/bindings'),
};
