# Hakimi WebUI

React/Vite operator console for the Hakimi Agent HTTP server. The layout follows the practical Hermes WebUI shape: sessions on the left, live chat in the center, and runtime/tool/control panels on the right.

## Current surface

- Live `/api/chat` turn submission with local transcript rendering.
- Recent `/api/sessions` list and `/api/sessions/{id}/messages` inspection.
- Runtime summaries from `/api/health`, `/api/status`, `/v1/capabilities`, `/api/mcp/servers`, `/api/credentials/pool`, and `/api/webhooks`.
- Tool and skill discovery from `/api/tools`, `/v1/toolsets`, and `/v1/skills`.
- Runtime config read/write through `/api/config`.
- Optional Bearer token storage for servers protected by `HAKIMI_WEBUI_PASSWORD`.

## Development

Start the Hakimi HTTP server separately, then run:

```sh
npm run dev
```

The Vite dev server proxies `/api/*` and `/v1/*` to `http://127.0.0.1:3005` without rewriting path prefixes, matching the Rust server router.

## Build

```sh
npm run lint
npm run build
```

`npm run build` emits the production bundle into `../crates/hakimi-webui/static/`
(`app.js`, `app.css`, `index.html` + copied `public/` assets) with stable filenames
and a `/static/` base. The Rust server embeds those files via `include_str!`
(`crates/hakimi-server/src/api.rs`), so the build output is committed and ships with
the binary — no node step is required in CI. Re-run `npm run build` and commit after
changing any frontend source.

## Remaining parity

- No xterm.js/WebSocket PTY terminal yet.
- `/api/chat` is shared server-agent chat; session-scoped chat is not implemented by the backend.
- Kanban APIs exist, but this WebUI does not yet include a full board/task management view.
- Runtime config writes are in-memory for the current server process unless the backend later persists them.
