# Worktree isolation strategy (Hakimi Studio Phase 3.5)
#
# Default: ON for parallel sub-agents (`Workspace::worktree_isolation_default() == true`).
#
# Layout
# ------
# Workspace root/
#   .worktrees/
#     <agent_id>/     # git worktree (preferred) or plain dir isolation
#       .hakimi-worktree  # marker when not a real git worktree
#
# API (`hakimi-workspace`)
# -----------------------
# - `Workspace::worktree_relative_path(agent_id)` → `.worktrees/<safe_id>`
# - `ensure_worktree(agent_id)` → absolute path
#     1. If path exists → reuse
#     2. If workspace is a git repo → `git worktree add -b hakimi/<id> .worktrees/<id> HEAD`
#     3. Else / on failure → create directory + `.hakimi-worktree` marker
# - `list_worktrees()` / `remove_worktree(agent_id)`
#
# Runtime policy
# --------------
# - Sub-agent tools should set cwd / workspace root to the worktree path.
# - Merge back is out of scope for this phase (manual `git merge` / PR).
# - Path jail still applies: worktrees live under the workspace root.
#
# Testing
# -------
# `cargo test -p hakimi-workspace worktree_dir_isolation_without_git`
