# Permission tiers (Hakimi Studio)

Runtime enforces **Controller vs Viewer** on session-scoped mutations.
Client-side `dangerConfirm` adds typed phrases for irreversible ops.

## Server roles (session attach)

| Role | Capabilities |
|------|----------------|
| **Controller** | Chat submit/preempt/cancel, runner handoff, workspace write/create/delete, checkpoint create/restore |
| **Viewer** | Session attach (subscribe), workspace list/read/grep, checkpoint list, devices list, ping |

When `session_id` is omitted (global workspace ops without a session), mutation is allowed (single-user desktop / tests). When `session_id` is set and the actor is viewer-only → `error.code = "viewer_readonly"`.

No device identity (unit tests without `hello`) → allow (legacy).

## Client danger confirm

| Op | Confirm |
|----|---------|
| File delete | `confirm` |
| Recursive delete | type `DELETE` |
| Checkpoint restore | type `RESTORE` |
| Cron delete | `confirm` |
| Runner handoff | `confirm` |

See `hakimi-webui/src/studio/dangerConfirm.ts`.

## Path deny policy (mutations)

`Workspace::assert_mutable_path` blocks **write / create / delete** on:

| Class | Examples |
|-------|----------|
| VCS / secrets dirs | `.git/**`, `.ssh/**`, `.gnupg/**`, `.aws/**`, `.kube/**`, `.hakimi/**` (except checkpoints) |
| Secret basenames | `.env`, `.env.local`, `id_rsa`, `*.pem`, `*.key`, `credentials.json` |

**Allowed:** `.hakimi/checkpoints/**` (Studio rewind store)

Error: `WorkspaceError::PathDenied` → Studio `error.code = workspace_error`.

Reads/list/grep are not blocked by this policy (path jail still applies).

## Future (not yet)

- Explicit permission levels: `read` / `write` / `danger` policy file
- Configurable allow/deny globs per session
- OS-native sandboxes (Tauri capabilities hardening)
