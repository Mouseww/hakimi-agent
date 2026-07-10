# JSON Formatter Plugin

A WASM plugin for Hakimi Agent that formats and validates JSON strings with pretty-printing.

## Features

- Validates JSON syntax
- Pretty-prints JSON with 2-space indentation
- Provides clear error messages for invalid JSON
- Lightweight (< 50KB after optimization)

## Build

Ensure you have the `wasm32-wasip1` target installed:

```bash
rustup target add wasm32-wasip1
```

Build the plugin:

```bash
cargo build --target wasm32-wasip1 --release
```

The compiled plugin will be at:
```
target/wasm32-wasip1/release/json_formatter_plugin.wasm
```

## Install

```bash
hakimi plugin install target/wasm32-wasip1/release/json_formatter_plugin.wasm
```

## Test

```bash
hakimi plugin test json-formatter
```

## Usage

### Input (minified JSON)

```json
{"name":"Alice","age":30,"emails":["alice@example.com","alice@work.com"],"active":true}
```

### Output (formatted)

```json
{
  "name": "Alice",
  "age": 30,
  "emails": [
    "alice@example.com",
    "alice@work.com"
  ],
  "active": true
}
```

### Error Handling

Input:
```
{"name": "Bob", "age": }
```

Output:
```
Error: Invalid JSON: expected value at line 1 column 22
```

## Development

Run tests:

```bash
cargo test
```

Check formatting:

```bash
cargo fmt --check
```

Run Clippy:

```bash
cargo clippy
```

## License

MIT
