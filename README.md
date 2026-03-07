# Runway

> Mac-native deployment control plane for AI-generated code.
> "Paste code. Pick a target. It's running."

Runway is a Tauri-based macOS app that lets developers deploy and manage code -- especially AI-generated code from Claude Code, Codex, Cursor -- to local machines, remote Linux servers, and (soon) AWS Lambda and Cloudflare Workers.

## Features

- **Paste & Deploy** -- Paste code, pick a runtime, hit Run. Zero config.
- **Remote Agents** -- Deploy a single Rust binary to any Linux server. Persistent execution history, agent-side scheduling, store-and-forward events, remote self-upgrade.
- **MCP Server** -- 17 tools via JSON-RPC 2.0. Claude Code can deploy, run, stop, and monitor projects programmatically.
- **CLI** -- Full command parity with the GUI and MCP server.
- **Runtime Detection** -- Auto-detects Python, Node, Go, Rust, Shell. Parses requirements.txt, package.json, go.mod, Cargo.toml.
- **Scheduling** -- Cron-like scheduling (`@every 5m`, `@hourly`, `30 9 * * *`). Agent runs jobs independently even when the app is offline.
- **Log Streaming** -- Real-time stdout/stderr via xterm.js terminal with ANSI color support.

## Architecture

```
mac-rundeck/
  src-tauri/         Tauri 2.0 app (Rust backend + React frontend)
  crates/
    runway-core/     Shared Rust library (projects, runtime, executor, gRPC client, SQLite)
    runway-agent/    Persistent execution agent (14 gRPC RPCs, SQLite, scheduler)
    runway-cli/      CLI interface
    runway-mcp/      MCP server (stdio transport)
  proto/             gRPC protobuf definitions
  src/               React 19 + TypeScript frontend
  docs/              Technical documentation
```

### Communication

- **Local**: Embedded agent via Unix Domain Socket (`~/.runway/agent.sock`)
- **Remote**: gRPC over HTTP/2 (port 50051) to persistent agent on Linux
- **MCP**: stdio transport (Claude Code spawns it)
- **LAN Discovery**: mDNS (`_runway._tcp.local.`)

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
cargo build -p runway-agent --release
sudo cp target/release/runway-agent /usr/local/bin/

# 3. Create systemd service
sudo tee /etc/systemd/system/runway-agent.service > /dev/null <<EOF
[Unit]
Description=Runway Agent
After=network.target

[Service]
Type=simple
User=$USER
ExecStart=/usr/local/bin/runway-agent --listen-all --port 50051
Restart=always
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now runway-agent
```

### Register Target in App

```bash
# Via CLI
runway targets add my-server --host 192.168.1.222 --port 50051
runway targets ping my-server

# Or use the Targets page in the GUI
```

### MCP Integration (Claude Code)

Add to your `.mcp.json`:
```json
{
  "mcpServers": {
    "runway": {
      "command": "cargo",
      "args": ["run", "-p", "runway-mcp"]
    }
  }
}
```

Then in Claude Code: "Deploy this script to my Linux server using Runway"

## Remote Agent

The remote agent (`runway-agent`) is a persistent Rust binary with:

- **SQLite store** (`~/.runway/agent.db`) -- 5 tables: deployments, executions, execution_logs, events, schedules
- **14 gRPC RPCs** -- Deploy, Execute, Stop, Health, Status, StreamLogs, GetExecutions, GetExecutionLogs, GetEvents, AckEvents, AddSchedule, RemoveSchedule, ListSchedules, Upgrade
- **Agent-side scheduler** -- Runs cron jobs every 30s, even when the app is disconnected
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

- [Technical Documentation](docs/index.html) -- Architecture, database schema, Tauri commands, protobuf, MCP tools
- [Remote Agent Reference](docs/remote-agent.html) -- Agent architecture, gRPC protocol, deployment, security

## Status

Phases 1-3 complete. Phase 4 (AWS Lambda + polish) is next.

## License

Proprietary. All rights reserved.
