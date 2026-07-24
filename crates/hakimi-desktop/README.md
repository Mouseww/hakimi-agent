# Hakimi Desktop

See [docs/hakimi-studio/DESKTOP.md](../../docs/hakimi-studio/DESKTOP.md).

```bash
# Headless (works on EL9)
cargo run -p hakimi-desktop -- --bind 127.0.0.1:3015

# Smoke
cargo run -p hakimi-desktop -- --once
cargo test -p hakimi-desktop

# Native window (needs webkit2gtk-4.1 — not on EL9)
cargo run -p hakimi-desktop --features gui
```
