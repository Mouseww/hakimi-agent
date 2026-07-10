# Snippet Store Plugin

Store and retrieve code snippets with metadata.

## Overview

This plugin demonstrates:
- Data structure management
- State handling in plugins
- Multi-language snippet storage

## Build

```bash
cargo build --target wasm32-wasip1 --release
```

## Usage

```bash
hakimi plugin install target/wasm32-wasip1/release/snippet_store_plugin.wasm
hakimi plugin execute snippet-store
```

## Example Output

```
📚 Snippet Store Plugin

Total Snippets: 3

1. hello-world-rust [rust]
   Basic Rust hello world
   Code:
   fn main() {
       println!("Hello, World!");
   }

2. factorial-python [python]
   Recursive factorial function
   Code:
   def factorial(n):
       return 1 if n <= 1 else n * factorial(n-1)
```

## Features

- Pre-loaded code snippet library
- Multiple programming languages
- Formatted code display
- Metadata tracking

## Future Enhancements

- Dynamic snippet addition/removal
- Search and filter capabilities
- Snippet categories and tags
- Export/import functionality
