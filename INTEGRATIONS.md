# Third-Party Integrations

Hakimi Agent supports multiple integration patterns to connect with external
services, databases, and tools. This guide covers all of them.

---

## MCP Servers

MCP (Model Context Protocol) servers are the primary way to extend Hakimi with
new capabilities. They run as child processes and communicate over JSON-RPC.

### One-Click Setup

Add any catalog entry to `~/.hakimi/config.yaml`:

```yaml
mcp_servers:
  filesystem:
    command: "npx"
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/"]

  github:
    command: "npx"
    args: ["-y", "@modelcontextprotocol/server-github"]
    env:
      GITHUB_TOKEN: "ghp_your_token_here"

  brave-search:
    command: "npx"
    args: ["-y", "@modelcontextprotocol/server-brave-search"]
    env:
      BRAVE_API_KEY: "BSA_your_key_here"
```

### Popular Servers

| Server | Category | Description | Env Vars |
|--------|----------|-------------|----------|
| **filesystem** | filesystem | Read/write files and directories | — |
| **github** | scm | Access repos, issues, PRs | `GITHUB_TOKEN` |
| **brave-search** | search | Web search with Brave API | `BRAVE_API_KEY` |
| **postgres** | database | Query PostgreSQL databases | `DATABASE_URL` |
| **puppeteer** | devtools | Browser automation | — |
| **memory** | devtools | Persistent knowledge graph | — |
| **fetch** | devtools | Make HTTP requests | — |
| **sqlite** | database | Query SQLite databases | — |
| **sequential-thinking** | devtools | Step-by-step reasoning | — |

### Browse the Catalog

From the REPL:

```
/plugins list              # Show installed plugins + available MCP servers
/plugins catalog           # List all catalog entries
/plugins catalog search github   # Search by keyword
/plugins catalog category database  # Filter by category
/plugins enable github     # Enable an MCP server (writes to config)
```

### Creating Custom MCP Servers

1. **Build your server** — any language that supports stdio JSON-RPC works.
   The server must implement the MCP protocol (initialize, tools/list,
   tools/call).

2. **Add to config**:
   ```yaml
   mcp_servers:
     my_server:
       command: "node"
       args: ["/path/to/my-server.js"]
       env:
         API_KEY: "your-key"
   ```

3. **Restart Hakimi** — tools from your server appear automatically.

Use the template at `templates/mcp-server-custom.yaml` as a starting point.

---

## HTTP Plugins

HTTP plugins let you wrap any REST API as a Hakimi tool — no server process
needed. Hakimi calls the endpoint directly.

### Quick Start

1. Copy a template to `~/.hakimi/plugins/`:
   ```bash
   cp templates/plugin-http-api.yaml ~/.hakimi/plugins/my_api.yaml
   ```

2. Edit the YAML — set your endpoint, method, headers, and parameters.

3. Restart Hakimi. Your tool appears in `/tools`.

### Template: HTTP API Plugin

```yaml
name: my_api
version: "1.0"
description: "My custom API plugin"
tools:
  - name: example_tool
    endpoint: "https://api.example.com/v1/action"
    method: POST
    description: "Description of what this tool does"
    headers:
      Authorization: "Bearer ${API_TOKEN}"
    parameters:
      type: object
      properties:
        input:
          type: string
          description: "Input parameter"
      required: ["input"]
```

### Template: Weather API

```yaml
name: weather
tools:
  - name: get_weather
    endpoint: "https://wttr.in/{city}?format=j1"
    method: GET
    description: "Get weather for a city"
    parameters:
      type: object
      properties:
        city:
          type: string
          description: "City name"
      required: ["city"]
```

### How It Works

- Hakimi scans `~hakimi/plugins/` for `.yaml`, `.yml`, and `.json` files.
- Each file defines one plugin with one or more tools.
- Tools become available to the agent like built-in tools.
- Environment variables in headers (e.g. `${API_TOKEN}`) are resolved at call time.

---

## Gateway Webhooks

The gateway lets external services push events to Hakimi via HTTP webhooks.

### Setup

1. Copy the template:
   ```bash
   cp templates/gateway-webhook.yaml ~/.hakimi/plugins/my_webhook.yaml
   ```

2. Configure:
   ```yaml
   name: my_webhook
   type: webhook
   port: 8080
   path: "/webhook"
   secret: "your-webhook-secret"
   ```

3. Start Hakimi with `--serve` to enable the HTTP API:
   ```bash
   hakimi --serve --addr 0.0.0.0:8080
   ```

### Use Cases

- **CI/CD notifications** — trigger agent actions on build completions
- **Monitoring alerts** — auto-respond to PagerDuty/Grafana alerts
- **Chat integrations** — bridge Slack/Discord/Telegram messages
- **Data pipelines** — process incoming data events

---

## Plugin Templates Reference

All templates are in the `templates/` directory:

| Template | Purpose |
|----------|---------|
| `plugin-http-api.yaml` | Generic HTTP API wrapper |
| `plugin-weather.yaml` | Weather API example |
| `mcp-server-custom.yaml` | Custom MCP server config |
| `gateway-webhook.yaml` | Webhook endpoint config |

---

## Troubleshooting

**MCP server won't start?**
- Make sure `npx` / `uvx` is installed and on your PATH.
- Check that required environment variables are set.
- Run the command manually to verify it works: `npx -y @modelcontextprotocol/server-github`

**Plugin not loading?**
- Ensure the YAML is valid (no syntax errors).
- Check that `~/.hakimi/plugins/` exists and contains your `.yaml` file.
- Look at the startup output for error messages.

**Need help?**
- Run `hakimi --doctor` for diagnostics.
- Check the logs: `RUST_LOG=debug hakimi`
