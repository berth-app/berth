# Berth

> Mac-native deployment control plane for AI-generated code.
> "Paste code. Pick a target. It's running."

Berth is a Tauri-based macOS app that lets developers deploy and manage code -- especially AI-generated code from Claude Code, Codex, Cursor -- to local machines, remote Linux servers, and (soon) AWS Lambda and Cloudflare Workers.

## Features

- **Paste & Deploy** -- Paste code, pick a runtime, hit Run. Zero config.
- **Environment Variables** -- Per-project env var storage with .env import, secret masking in logs.
- **Service Mode** -- Keep API servers, bots, and workers running. Auto-restart on crash with exponential backoff.
- **Docker Compose** -- Drop in a docker-compose.yml. Berth detects it, runs all services, streams logs.
- **Public URLs** -- Click "Publish" on any running service → instant public URL. Pluggable providers: Cloudflare Quick Tunnel (free), ngrok, or custom command. Berth orchestrates, you pick the tunnel.
- **Remote Agents** -- Deploy a single Rust binary to any Linux server. Persistent execution history, agent-side scheduling, store-and-forward events. Communicates via NATS — no direct network connection or open ports needed.
- **Agent Self-Upgrade** -- Cloudflared-model upgrade: atomic binary swap, exit(42), systemd restart, 30s probation with automatic rollback.
- **MCP Server** -- 19 tools via JSON-RPC 2.0. Claude Code can deploy, run, stop, publish, and monitor projects programmatically.
- **CLI** -- Full command parity with the GUI and MCP server.
- **Runtime Detection** -- Auto-detects Python, Node, Go, Rust, Shell, Docker Compose. Parses requirements.txt, package.json, go.mod, Cargo.toml, docker-compose.yml.
- **Scheduling** -- Cron-like scheduling (`@every 5m`, `@hourly`, `30 9 * * *`). Agent runs jobs independently even when the app is offline.
- **Log Streaming** -- Real-time stdout/stderr via xterm.js terminal with ANSI color support.

## Architecture

```
mac-berth/
  src-tauri/         Tauri 2.0 app (Rust backend + React frontend)
  crates/
    berth-core/     Shared Rust library (projects, runtime, executor, gRPC client, SQLite, tunnel)
    berth-agent/    Persistent execution agent (16 gRPC RPCs, SQLite, scheduler)
    berth-cli/      CLI interface
    berth-mcp/      MCP server (stdio transport)
  proto/             gRPC protobuf definitions
  src/               React 19 + TypeScript frontend
  docs/              Technical documentation
```

### Communication

- **Local**: Embedded agent via Unix Domain Socket (`~/.berth/agent.sock`)
- **Remote (NATS)**: All remote agent communication routed through NATS (Synadia Cloud). Neither desktop nor agent needs to expose ports. Works behind NAT, firewalls, and across different networks. `AgentTransport` trait abstracts transport selection per target.
- **Remote (gRPC)**: Fallback for targets without NATS — gRPC over HTTP/2 (port 50051)
- **MCP**: stdio transport (Claude Code spawns it)
- **LAN Discovery**: mDNS (`_berth._tcp.local.`)

## Quick Start

### Run the App (Development)

```bash
# Prerequisites: Rust, Node.js, protoc
cargo tauri dev
```

### Deploy a Remote Agent

```bash
# On the Linux server:
# 1. Install Rust + protoc
sudo apt install -y protobuf-compiler build-essential
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

# 2. Build and install
cargo build -p berth-agent --release
sudo cp target/release/berth-agent /usr/local/bin/

# 3. Create systemd service
sudo tee /etc/systemd/system/berth-agent.service > /dev/null <<EOF
[Unit]
Description=Berth Agent
After=network.target

[Service]
Type=simple
User=$USER
ExecStart=/usr/local/bin/berth-agent --listen-all --port 50051 --nats-url tls://connect.ngs.global --nats-creds /path/to/nats.creds --nats-agent-id my-server
Restart=always
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now berth-agent
```

### Register Target in App

```bash
# Via CLI
berth targets add my-server --host 192.168.1.100 --port 50051
berth targets ping my-server

# Or use the Targets page in the GUI
```

### Publish a Service to a Public URL

```bash
# Prerequisite: brew install cloudflared

# Via CLI
berth publish my-api --port 8080
# Published: https://random-words.trycloudflare.com

berth unpublish my-api

# Or click "Publish" in the project toolbar in the GUI
```

### MCP Integration (Claude Code)

Add to your `.mcp.json`:
```json
{
  "mcpServers": {
    "berth": {
      "command": "cargo",
      "args": ["run", "-p", "berth-mcp"]
    }
  }
}
```

Then in Claude Code: "Deploy this script to my Linux server using Berth"

## Remote Agent

The remote agent (`berth-agent`) is a persistent Rust binary with:

- **SQLite store** (`~/.berth/agent.db`) -- 5 tables: deployments, executions, execution_logs, events, schedules
- **16 gRPC RPCs** -- Deploy, Execute, Stop, Health, Status, StreamLogs, GetExecutions, GetExecutionLogs, GetEvents, AckEvents, AddSchedule, RemoveSchedule, ListSchedules, Upgrade, Publish, Unpublish
- **NATS command channel** -- All RPCs available over NATS relay. Desktop sends commands to `berth.<agent_id>.cmd.<type>`, agent responds via `berth.<agent_id>.resp.<request_id>`. Zero inbound ports required.
- **Agent-side scheduler** -- Runs cron jobs every 30s, even when the app is disconnected. Triggers macOS notifications on the desktop via NATS events.
- **Store-and-forward events** -- Agent queues events, app polls when connected
- **Remote upgrade** -- Upload new binary via gRPC, agent verifies and restarts via systemd
- **Dependency install** -- Deploy RPC auto-runs `pip install`, `npm install`, `go mod download`

See [docs/remote-agent.html](docs/remote-agent.html) for full technical reference.

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Desktop App | Tauri 2.0 + React 19 + TypeScript |
| Styling | Tailwind CSS |
| Terminal | xterm.js |
| Rust Backend | tokio + tonic (gRPC) + axum (HTTP) |
| Database | SQLite via rusqlite (bundled) |
| Agent | Rust single binary, systemd service |
| MCP | JSON-RPC 2.0 stdio transport |

## Documentation

- [Technical Documentation](https://docs.getberth.dev) -- Architecture, database schema, Tauri commands, protobuf, MCP tools
- [Remote Agent Reference](https://docs.getberth.dev/remote-agent.html) -- Agent architecture, gRPC protocol, deployment, security

## License

[Business Source License 1.1](LICENSE) — free to use, modify, and self-host. Converts to Apache 2.0 on 2029-03-10. See LICENSE for details.
