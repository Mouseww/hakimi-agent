# Hakimi Studio Protocol v1

> Status: Phase 2 (multi-device hub + replay gap)  
> Transport: JSON over WebSocket (+ HTTP helpers)  
> Encoding: UTF-8 JSON text frames

## Agent execution

- Queue / preempt / session state live in `StudioRuntime` (`hakimi-studio-api`).
- Turn execution is pluggable via `AgentHost`:
  - **Unit tests / default:** `MockAgentHost` (deterministic stream + mock tool).
  - **Production server:** `CoreAgentHost` clones shared `AIAgent`, attaches request-local streaming callback, maps:
    - text tokens → `message.delta`
    - `\u{001e}hakimi_tool:…` → `tool.started`
    - `\u{001e}hakimi_tool_result:…` → `tool.completed`
    - chat result → `message.completed` + `session.ended`
  - Preempt / cancel: `Notify` + agent `interrupt` AtomicBool; reason `preempted` | `done` | `error`.
- **Never** store streaming callbacks on the process-shared agent (SSE hang / busy composer).

## Multi-device (Phase 2)

| Concept | Behavior |
|---------|----------|
| Device register | `hello` → `device.registered`; hub may require `token` |
| List devices | `devices.list` → `devices.listed` |
| Attach role | `session.attach` with `role: controller\|viewer` |
| Viewer | may attach + receive events; chat submit/cancel/preempt/handoff → `error` code `viewer_readonly` |
| Active Runner | `session.created` / `runner.changed.active_runner_device_id` |
| Handoff | `runner.handoff` → promote target device to controller + `runner.changed` |
| Replay | `session.attach.after_seq` replays window; gap → `session.reset` (client must resnapshot) |
| Hub binary | `hakimi-hub` on `/v1/studio` + `/health`; **no tool execution, no provider keys** |
| Per-connection identity | WS binds `device_id` from `hello`; subsequent cmds use `handle_command_as(actor)` |
| Hub modes | `embedded` (in-process StudioRuntime demo) · `relay` (pure fan-out) |
| Pure-relay commands | `worker_publish` (worker→hub events) · `worker_dispatch` (hub→runner, not a StudioCommand) |

## Transport

| Channel | Path | Role |
|---------|------|------|
| Control + events | `WS /v1/studio` | commands in, events out |
| Health | `GET /v1/studio/health` | liveness |
| (later) Terminal | `WS /v1/studio/terminal` | raw PTY bytes |

### Handshake (WS)

1. Client connects to `/v1/studio`.
2. Optional first message: `{ "type": "hello", "device_id": "...", "token": "..." }`.
3. Server replies with `hello.ok` event (seq may be 0 / global).
4. Client may `session.create` / `session.attach` / `chat.submit`.

### Framing

- One JSON object per WebSocket text message.
- Commands: `StudioCommand` (client → server/runner).
- Events: `StudioEventEnvelope` = `{ "seq": u64, "session_id": string|null, "event": StudioEvent }`.

## Product rules (locked)

- Default runner: **local** (prefer local device); switchable via `runner.handoff`.
- Hub: public self-host.
- Concurrent submits: **queue + preempt** (`chat.submit` queues; `chat.preempt` cancels current run then runs next).
- UI: Workspace primary; Office optional.

## Commands (`StudioCommand`)

Discriminated by `"type"`:

| type | fields | notes |
|------|--------|-------|
| `hello` | `device_id`, `token?`, `device_name?`, `kind?` | kind: `desktop\|web\|server\|cli` |
| `session.create` | `workspace_id?`, `title?`, `prefer_runner?` | prefer_runner: `local\|server\|device:<id>` |
| `session.attach` | `session_id`, `after_seq?`, `role?` | role: `viewer\|controller` (default controller) |
| `session.list` | `limit?` | |
| `chat.submit` | `session_id`, `text`, `client_request_id`, `preempt?` | if busy & !preempt → queued |
| `chat.cancel` | `session_id`, `run_id?` | cancel current or specific |
| `chat.preempt` | `session_id`, `text`, `client_request_id` | cancel current then submit |
| `runner.handoff` | `session_id`, `to_device_id`, `from_device_id?` | change Active Runner |
| `devices.list` | | registered devices |
| `workspace.list` | `session_id?`, `path` | jailed list |
| `workspace.read` | `session_id?`, `path` | |
| `workspace.write` | `session_id?`, `path`, `content` | |
| `workspace.create` | `session_id?`, `path`, `is_dir?` | |
| `workspace.delete` | `session_id?`, `path`, `recursive?` | |
| `workspace.grep` | `session_id?`, `path`, `pattern`, `limit?` | |
| `checkpoint.create` | `session_id?`, `label?`, `paths?` | file snapshot under `.hakimi/checkpoints/` |
| `checkpoint.list` | `session_id?` | |
| `checkpoint.restore` | `session_id?`, `checkpoint_id` | **client must danger-confirm** |
| `worker_publish` | `events[]` | worker→hub only (pure-relay) |
| `ping` | `nonce?` | |

## Events (`StudioEvent`)

| type | meaning |
|------|---------|
| `hello.ok` | handshake accepted |
| `hello.error` | auth/protocol error |
| `session.created` | new session id + runner |
| `session.snapshot` | history + last_seq for attach |
| `session.listed` | session summaries |
| `queue.updated` | queue depth / items |
| `run.started` | run_id, client_request_id |
| `run.queued` | waiting behind current |
| `run.preempted` | previous run cancelled for preempt |
| `message.delta` | streaming assistant text |
| `message.completed` | final assistant message |
| `tool.started` | name, call_id |
| `tool.completed` | call_id, ok |
| `agent.progress` | todos / fleet summary |
| `subagent.spawned` | child id, goal |
| `subagent.completed` | child id, summary |
| `runner.changed` | active_runner device (+ from_device_id?) |
| `device.registered` | device joined hub/runtime |
| `devices.listed` | device summaries |
| `session.reset` | after_seq gap; resync from snapshot |
| `error` | recoverable error |
| `pong` | ping reply |
| `session.ended` | terminal for a run (not whole session) |
| `workspace.listed` | dir entries |
| `workspace.content` | file text |
| `workspace.written` / `created` / `deleted` | mut ops ack |
| `workspace.grep_result` | hits |
| `checkpoint.created` / `checkpoints.listed` / `checkpoint.restored` | rewind primitive |

## Pure-relay worker wiring

| Side | Env / API |
|------|-----------|
| Hub | `hakimi-hub --mode relay` (or `HAKIMI_HUB_MODE=relay`) |
| Worker | `HAKIMI_HUB_URL=ws://host:3010/v1/studio` on hakimi-server |
| Optional | `HAKIMI_HUB_TOKEN`, `HAKIMI_HUB_DEVICE_ID`, `HAKIMI_HUB_DEVICE_NAME` |
| Dispatch | Hub sends `{type:"worker_dispatch", actor_device_id?, command}` to Active Runner |
| Publish | Runner replies with `worker_publish` envelopes; hub re-seq + fan-out |

## Ordering & recovery

- `seq` is **per-session** monotonic `u64` starting at 1.
- Attach with `after_seq` → server replays `(after_seq, last]` from bounded window.
- Gap too large → `session.snapshot` reset (client rebuilds from snapshot).
- `client_request_id` is idempotent for 24h on the runner (dedupe submits).

## Queue + preempt semantics

```
idle  --submit--> running
running --submit(no preempt)--> queue += 1; emit queue.updated + run.queued
running --preempt|submit(preempt=true)--> cancel run; emit run.preempted; start new
running --run ends--> pop queue head if any
```

## Active Runner

- Exactly one `active_runner_device_id` per session.
- Controllers may submit; viewers only receive events.
- `runner.handoff` requires current runner online OR force flag (Phase 2).

## Versioning

- Protocol version field in hello: `protocol_version: 1`.
- Backward-incompatible changes bump major and path `/v2/studio`.
