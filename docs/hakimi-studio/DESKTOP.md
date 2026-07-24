# Hakimi Studio Desktop (`hakimi-desktop`)

Local-first desktop shell for Hakimi Studio.

## Architecture

```
┌─────────────────────────────────────────┐
│  hakimi-desktop binary                  │
│  ┌───────────────────────────────────┐  │
│  │ Embedded Axum backend             │  │
│  │  • WebUI static (/static/*)       │  │
│  │  • Studio WS (/v1/studio)         │  │
│  │  • Studio health                  │  │
│  └───────────────────────────────────┘  │
│           ▲                             │
│           │ same-origin                 │
│  ┌────────┴──────────┐                  │
│  │ WebUI (Workspace) │  headless browser│
│  │ or Tauri webview  │  / --open / gui  │
│  └───────────────────┘                  │
└─────────────────────────────────────────┘
```

- **Agent loop stays in Rust** (`StudioRuntime` via `hakimi-server::StudioState`).
- UI is shell only (existing React WebUI).
- Hub pure-relay remains optional (`HAKIMI_HUB_URL` on full server); desktop defaults to local runner.

## Features

| Cargo feature | Behavior |
|---------------|----------|
| *(default)* | Headless: start backend, print URL, wait for Ctrl-C |
| `gui` | Tauri 2 window navigates to local backend |

### Linux GUI prerequisite

Tauri 2 requires **webkit2gtk-4.1**. EL9 / RHEL9 only ship **webkit2gtk-4.0**, so `--features gui` will not link on this host. Use:

- Fedora 39+
- Ubuntu 22.04+
- GitHub `ubuntu-latest` CI image

Headless backend + tests work on EL9 without WebKit 4.1.

## Usage

```bash
# Headless local Studio (ephemeral port)
cargo run -p hakimi-desktop

# Fixed port + open browser
cargo run -p hakimi-desktop -- --bind 127.0.0.1:3015 --open

# Workspace root
cargo run -p hakimi-desktop -- --workspace /path/to/project

# Native window (needs webkit2gtk-4.1)
cargo run -p hakimi-desktop --features gui
```

Env:

| Variable | Meaning |
|----------|---------|
| `HAKIMI_DESKTOP_WORKSPACE` | Default workspace root |
| `RUST_LOG` | tracing filter |

## Tests

```bash
cargo test -p hakimi-desktop --lib
```

Covers: health JSON, index HTML, `/static/app.js`.

## Packaging (Phase 4.2) — GitHub Actions cloud matrix

Workflow: [`.github/workflows/desktop.yml`](../../.github/workflows/desktop.yml)

| Job | Runners | Output |
|-----|---------|--------|
| **headless** | ubuntu-22.04, windows-latest, macos-latest | `hakimi-desktop` binary (`--once` smoke) |
| **gui** | same matrix + WebKit/WebView deps | `cargo build --features gui` + `cargo tauri build` installers |
| **release-assets** | on `v*` tags only | attaches artifacts to GitHub Release |

Triggers: `workflow_dispatch`, push/PR paths under desktop/studio, tags `v*`.

```bash
# Manually run packaging on GitHub
gh workflow run desktop.yml
# Watch
gh run list --workflow=desktop.yml --limit 5
gh run watch <id>
```

`tauri.conf.json` targets: `deb`, `appimage`, `msi`, `dmg`.

Local GUI still requires webkit2gtk-4.1 (not EL9). Prefer cloud packaging.

## Layout

```
crates/hakimi-desktop/
  Cargo.toml          # features: gui
  tauri.conf.json
  capabilities/
  icons/
  build.rs
  src/
    main.rs           # CLI
    lib.rs
    backend.rs        # Axum Studio + static
    gui.rs            # feature=gui only
```
