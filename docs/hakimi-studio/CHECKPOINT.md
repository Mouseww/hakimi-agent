# Checkpoint / Rewind (Hakimi Studio Phase 5 primitive)

File-level snapshots under the path-jailed workspace. Full agent-action rewind
and ACP remain open; this layer is the durable restore primitive.

## Storage

```
Workspace root/
  .hakimi/
    checkpoints/
      <cp-id>/
        manifest.json
        files/            # mirrored relative paths
          note.txt
```

## Library (`hakimi-workspace`)

| Method | Description |
|--------|-------------|
| `create_checkpoint(label, paths)` | Snapshot selected paths (empty = top-level non-hidden) |
| `list_checkpoints()` | Newest-first list |
| `restore_checkpoint(id)` | Overwrite workspace files from snapshot |

Skipped: hidden dirs, `target/`, `node_modules/`, files larger than `max_read_bytes`.

## Protocol (Studio v1)

**Commands**

- `checkpoint_create` `{ session_id?, label?, paths? }`
- `checkpoint_list` `{ session_id? }`
- `checkpoint_restore` `{ session_id?, checkpoint_id }` — **requires client danger confirm**

**Events**

- `checkpoint_created` / `checkpoints_listed` / `checkpoint_restored` with `CheckpointView`

## UI policy

- Status bar **CP** toggle → `StudioCheckpointPanel`
- Create: optional label; paths default to current open file or top-level
- Restore uses `Danger.restoreCheckpoint(id)` (`dangerConfirm.ts`) — typed `RESTORE`
- System chat line on create/restore; restore refreshes open file + tree

## Auto-checkpoint (runtime)

Before `workspace_write` / `workspace_delete` on an existing path:

1. `create_checkpoint(label="auto:pre-write|pre-delete:<path>", paths=[path])`
2. Emit `checkpoint_created` (best-effort; skip missing / `.hakimi/` paths)
3. Proceed with mutation

New-file writes that fail snapshot are silent (no block).

## Tests

```bash
cargo test -p hakimi-workspace checkpoint_create_and_restore
cargo test -p hakimi-studio-api write_existing_file_auto_checkpoints
```
