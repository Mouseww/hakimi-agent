# Markdown Plugin

Convert Markdown to formatted text output.

## Overview

This plugin demonstrates:
- Text processing and parsing
- Simple format conversion
- String manipulation in WASM

## Build

```bash
cargo build --target wasm32-wasip1 --release
```

## Usage

```bash
hakimi plugin install target/wasm32-wasip1/release/markdown_plugin.wasm
hakimi plugin execute markdown-plugin
```

## Example

Converts Markdown headings, lists, and emphasis to formatted text:

- `# Title` → `=== Title ===`
- `## Subtitle` → `--- Subtitle ---`
- `- Item` → `  • Item`

## Future Enhancements

- Full HTML conversion with pulldown-cmark
- Support for tables, code blocks, and links
- Custom CSS styling options
