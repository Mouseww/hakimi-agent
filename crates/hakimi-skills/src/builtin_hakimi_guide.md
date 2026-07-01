# Hakimi Agent Platform Guide

You are running inside Hakimi Agent. This guide covers everything you need to help
users configure, operate, and troubleshoot their Hakimi instance.

## Architecture Overview

Hakimi is a Rust-native multi-agent AI platform. Key components:

- **Personas**: isolated agent identities, each with its own model, system prompt, skills, memory, and session database
- **Gateway**: connects to external chat platforms (Telegram, QQ, WeChat, Discord, Slack, etc.)
- **Skills**: reusable prompt templates in markdown that extend agent capabilities
- **Tools/Toolsets**: executable actions (shell, file I/O, web search, code analysis, etc.)
- **WebUI**: browser-based operator console at the configured HTTP port
- **Cron**: scheduled recurring tasks with optional delivery to chat platforms
- **Memory**: per-persona persistent knowledge stored as markdown files

## Configuration File

All configuration lives in `~/.hakimi/config.yaml`. Every section has sensible defaults.

### Model Configuration

```yaml
model:
  default: "claude-sonnet-4-20250514"    # or any OpenAI/Anthropic/OpenRouter model
  provider: "auto"                        # auto | openrouter | anthropic | openai
  base_url: ""                            # custom API endpoint (empty = provider default)
  api_key: ""                             # API key (or set env: OPENROUTER_API_KEY, ANTHROPIC_API_KEY, OPENAI_API_KEY)
  api_mode: ""                            # auto | chat_completions | responses | anthropic_messages
  context_length: 0                       # 0 = auto-detect from model
```

### Agent Settings

```yaml
agent:
  max_turns: 90                 # max tool-calling iterations per conversation
  system_prompt: ""             # global system prompt override
  reasoning_effort: ""          # low | medium | high
  skills_path: ""               # custom skills dir (default: ~/.hakimi/skills/)
  disabled_toolsets: []          # toolset names to disable
  save_trajectories: false       # save conversations as JSONL for training
  trajectory_dir: ""             # output directory for trajectories
```

### Terminal / Sandbox

```yaml
terminal:
  env_type: "local"             # local | docker | ssh
  cwd: "."                      # working directory
  timeout: 60                   # command timeout in seconds
  docker_image: ""              # image for docker mode
  docker_volumes: []             # host:container volume mounts
```

### Delegation (Sub-agents)

```yaml
delegation:
  max_iterations: 45            # max turns per delegated sub-agent
  model: ""                     # sub-agent model (empty = inherit parent)
  provider: ""                  # sub-agent provider (empty = inherit)
```

### Memory

```yaml
memory:
  enabled: true
  path: ""                      # custom dir (default: ~/.hakimi/memory/)
```

### Compression

```yaml
compression:
  enabled: true
  threshold: 0.50               # context usage ratio to trigger compression
  target_ratio: 0.20            # target compression ratio
  engine: "smart"               # smart | simple | llm
```

### Display

```yaml
display:
  compact: false
  streaming: true
  language: "en"                # en | zh | ja | ko | ...
  skin: "default"
```

### WebUI

```yaml
webui:
  password: ""                  # Bearer token for WebUI auth (empty = no auth)
```

### Credential Pools

For multi-key rotation (round_robin, fill_first, random, least_used):

```yaml
credential_pools:
  openrouter:
    strategy: "round_robin"
    credentials:
      - api_key: "sk-or-..."
        priority: 10
      - api_key: "sk-or-..."
        priority: 5
```

### MCP Servers

```yaml
mcp_servers:
  my-server:
    command: "npx"
    args: ["-y", "@my/mcp-server"]
    env:
      API_KEY: "..."
```

### Embedding

```yaml
embedding:
  enabled: true
  provider: "openai-compatible"
  model: "BAAI/bge-m3"
  dimension: 1024
  base_url: ""                  # empty = same as model.base_url
  api_key: ""                   # empty = same as model API key
```

### Voice / TTS

```yaml
voice:
  provider: "openai"            # openai | elevenlabs
  model: "tts-1"
  voice: "alloy"                # alloy | onyx | nova | ...
  transcription_model: "whisper-1"
  record_key: "ctrl+b"          # push-to-talk key
```

## Gateway Platform Configuration

The gateway connects Hakimi to external chat platforms. Each platform requires
`enabled: true` and platform-specific credentials.

### Global Gateway Settings

```yaml
gateways:
  allow_all: false                       # true = allow all users
  allowed_users: ["user_id_1"]           # global allowlist
  filter_silence_narration: true         # drop "(silent)" messages
  busy_input_mode: "parallel"             # parallel | queue | interrupt
```

### Telegram

```yaml
gateways:
  telegram:
    bot_token: "123456:ABC-DEF..."       # from @BotFather
    allowed_users: [12345678]            # Telegram numeric user IDs (empty = allow all)
```

The Telegram adapter's `bot_id` for channel binding defaults to `"telegram_bot"`.
It is set in the CLI entry point, not in config.yaml directly.

Channel binding: `telegram:telegram_bot`

### QQ Bot (QQ Official Robot)

```yaml
gateways:
  qqbot:
    enabled: true
    bot_id: "qqbot"                      # custom instance name for binding
    app_id: "your_app_id"               # from QQ Bot platform
    client_secret: "your_secret"         # from QQ Bot platform
    home_channel: ""                     # default send target (optional)
    default_chat_type: "c2c"             # c2c | group
    markdown_support: true               # send markdown payloads
```

Channel binding: `qqbot:qqbot` (or whatever you set as `bot_id`)

### WeChat via ClawBot (HTTP Bridge)

```yaml
gateways:
  clawbot:
    enabled: true
    mode: "http_bridge"                  # http_bridge | weclawbot_api
    bot_id: "clawbot"                    # custom instance name
    base_url: "http://localhost:8081"    # ClawBot HTTP bridge URL
    poll_path: "/message/SyncMessage"
    send_path: "/message/SendTextMessage"
    poll_interval_ms: 3000
```

Channel binding: `clawbot:clawbot`

### WeChat via iLink (Native)

```yaml
gateways:
  weixin:
    enabled: true
    bot_id: "weixin"                     # custom instance name
    base_url: "https://ilinkai.weixin.qq.com"
    token: ""                            # seed bot_token if available
    home_channel: ""                     # default send target
    login_notify_platform: "telegram"    # where to send login QR
    login_notify_bot_id: "telegram_bot"
    login_notify_chat_id: "12345678"
```

Channel binding: `weixin:weixin`

### Discord

```yaml
gateways:
  discord:
    enabled: true
    bot_id: "discord"                    # custom instance name
    token: "your_discord_bot_token"      # from Discord Developer Portal
    channel_id: "1234567890"             # default channel (optional)
```

Channel binding: `discord:discord`

### Slack

```yaml
gateways:
  slack:
    enabled: true
    bot_id: "slack"                      # custom instance name
    token: "xoxb-your-slack-token"       # Bot User OAuth Token
    channel_id: "C01234567"              # default channel (optional)
```

Channel binding: `slack:slack`

### DingTalk

```yaml
gateways:
  dingtalk:
    enabled: true
    # (platform-specific fields - check dingtalk adapter docs)
```

### Feishu / Lark

```yaml
gateways:
  feishu:
    enabled: true
    # (platform-specific fields - check feishu adapter docs)
```

### WeCom (Enterprise WeChat)

```yaml
gateways:
  wecom:
    enabled: true
    # (platform-specific fields - check wecom adapter docs)
```

### Other Platforms

All follow the same pattern: enable in config.yaml, set credentials, and
create a channel binding. Supported: Email, WhatsApp, Signal, Matrix,
Mattermost, SMS, BlueBubbles (iMessage), Home Assistant, Webhook, Microsoft Graph.

## Persona Management

Personas are isolated agent identities. Each has its own:
- Model and system prompt
- Skills (loaded from `~/.hakimi/agents/<id>/skills/`)
- Memory (at `~/.hakimi/agents/<id>/memory/`)
- Session database
- Channel bindings

### File Structure

```
~/.hakimi/
  config.yaml                    # main config
  agents/
    registry.yaml                # persona index
    default/
      persona.yaml               # default persona config
      skills/                    # persona-specific skills
      memory/                    # persona-specific memory
        MEMORY.md                # memory index
    coder/
      persona.yaml
      skills/
      memory/
  skills/                        # global shared skills
  memory/                        # global memory (used by default persona)
    MEMORY.md
```

### persona.yaml Format

```yaml
id: coder
name: "Coding Assistant"
avatar: "🧑‍💻"
description: "Specialized coding agent"
model: "claude-opus-4-8"             # empty = inherit default
reasoning_effort: "high"              # low | medium | high | empty
system_prompt: "You are a coding expert..."
enabled_skills:
  - tdd
  - systematic-debugging
bindings:
  - "telegram:devbot"
  - "slack:coder-bot"
is_default: false
addressable: true                     # allow other agents to consult
```

### Channel Bindings

A binding maps a `platform:bot_id` pair to a specific persona. Format:

```
<platform>:<instance_name>
```

- `platform` is the adapter name: telegram, qqbot, clawbot, weixin, discord, slack, etc.
- `instance_name` is the `bot_id` value configured in the gateway config section.
  It is a custom label, NOT a platform token or numeric ID.

Example: if your config.yaml has `gateways.qqbot.bot_id: "my_qq"`, then the
binding key is `qqbot:my_qq`.

When an inbound message arrives with no matching binding, it falls back to the
default persona.

### Managing Personas via WebUI

1. Open the WebUI (default: http://localhost:port/static/)
2. Left rail shows personas; click the gear icon to edit
3. Click "+" to create a new persona
4. Settings page shows channel bindings with CRUD controls

### Managing Personas via API

```
GET    /api/agents              # list all personas
POST   /api/agents              # create persona
GET    /api/agents/{id}         # get persona config
PATCH  /api/agents/{id}         # update persona (partial)
DELETE /api/agents/{id}         # delete persona (cannot delete default)
GET    /api/agents/{id}/memory  # read persona memory
GET    /api/agents/{id}/skills  # list persona skills
```

## Cron / Scheduled Tasks

```yaml
# Create via API:
POST /api/cron/jobs
{
  "name": "Weekly review",
  "schedule": "0 9 * * 1",           # standard cron expression
  "prompt": "Review all open PRs",
  "skills": ["code-review"],
  "deliver": "slack:support"          # optional: send result to platform
}
```

API endpoints:
```
GET    /api/cron/jobs           # list jobs
POST   /api/cron/jobs           # create job
DELETE /api/cron/jobs/{id}      # delete job
POST   /api/cron/jobs/{id}/pause
POST   /api/cron/jobs/{id}/resume
POST   /api/cron/jobs/{id}/run  # trigger immediately
```

## Skills System

Skills are markdown files with YAML frontmatter in `~/.hakimi/skills/` (global)
or `~/.hakimi/agents/<id>/skills/` (per-persona).

### Skill Format

```markdown
---
name: my-skill
description: What this skill does
trigger: when the user asks about X
tags:
  - coding
  - testing
phases:
  - analyze
  - validate
ttl_steps: 5
max_context_chars: 1200
---
# Skill Content
Instructions for the agent when this skill is active...
```

Skills are dynamically activated based on conversation context and evicted
when no longer relevant.

## Tools and Toolsets

Built-in toolsets include:
- **shell**: execute commands in terminal
- **file**: read, write, search files
- **web**: fetch URLs, search the web
- **delegate**: spawn sub-agents for parallel work
- **kanban**: task/project management board
- **knowledge**: semantic search over indexed documents

Disable specific toolsets:
```yaml
agent:
  disabled_toolsets: ["shell"]
```

## API Reference (Key Endpoints)

### Chat
```
POST /api/chat                       # chat with default persona
POST /api/agents/{id}/chat           # chat with specific persona
POST /api/agents/{id}/chat/stream    # streaming chat (SSE)
```

### Sessions
```
GET    /api/sessions                 # list recent sessions
GET    /api/sessions/{id}/messages   # get session messages
DELETE /api/sessions/{id}            # delete session
GET    /api/sessions/search?q=...    # search sessions
```

### Async Runs
```
POST   /v1/runs                      # submit async task
GET    /v1/runs/{id}                 # poll status
GET    /v1/runs/{id}/events          # stream events (SSE)
POST   /v1/runs/{id}/stop            # cancel
```

### System
```
GET  /api/health                     # health check
GET  /api/status                     # dashboard status
GET  /v1/capabilities                # feature flags
GET  /v1/tools                       # list tools
GET  /v1/skills                      # list skills
GET  /api/bindings                   # channel binding map
```

## Common User Tasks

### "How do I connect a Telegram bot?"
1. Get a bot token from @BotFather on Telegram
2. Add to `~/.hakimi/config.yaml`:
   ```yaml
   gateways:
     telegram:
       bot_token: "123456:ABC-..."
   ```
3. Restart Hakimi or use WebUI gateway restart button
4. (Optional) Create a channel binding to route to a specific persona

### "How do I connect QQ Bot?"
1. Register at the QQ Bot platform, get app_id and client_secret
2. Add to `~/.hakimi/config.yaml`:
   ```yaml
   gateways:
     qqbot:
       enabled: true
       app_id: "your_app_id"
       client_secret: "your_secret"
   ```
3. Restart Hakimi

### "How do I connect WeChat?"
Two modes are available:

**ClawBot HTTP Bridge** (third-party bridge):
```yaml
gateways:
  clawbot:
    enabled: true
    mode: "http_bridge"
    base_url: "http://localhost:8081"
```

**iLink Native** (official WeChat iLink API):
```yaml
gateways:
  weixin:
    enabled: true
```
On first launch, a login QR code will be generated. Configure
`login_notify_platform` to receive it on another platform.

### "How do I create a new persona?"
Via WebUI: click "+" in the left persona rail, fill in the form, save.

Via API:
```
POST /api/agents
{
  "id": "coder",
  "name": "Coding Assistant",
  "system_prompt": "You are a coding expert...",
  "model": "claude-opus-4-8",
  "bindings": ["telegram:devbot"]
}
```

Via file: create `~/.hakimi/agents/coder/persona.yaml` with the persona config.

### "How do I route a platform to a specific persona?"
Add a channel binding. The binding format is `platform:bot_id` where `bot_id`
matches the `bot_id` field in that platform's gateway config.

Via WebUI: Settings page > Channel bindings > Add binding
Via API: `PATCH /api/agents/{id}` with `{"bindings": ["telegram:devbot"]}`
Via file: add to the persona's `bindings` list in `persona.yaml`

### "How do I change the model?"
Global: edit `model.default` in `~/.hakimi/config.yaml`
Per-persona: edit the persona's `model` field (WebUI or API)
Per-persona via WebUI: click persona gear icon > Model field

### "How do I add a skill?"
Create a `.md` file with YAML frontmatter in `~/.hakimi/skills/` (global) or
`~/.hakimi/agents/<id>/skills/` (per-persona).

### "How do I set up scheduled tasks?"
Via API: `POST /api/cron/jobs` with schedule, prompt, and optional delivery target.
Via WebUI: (if cron UI is available in the workspace panel)
