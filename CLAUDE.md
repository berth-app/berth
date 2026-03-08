# Runway - Project Instructions

> Mac-native deployment control plane for AI-generated code.
> "Paste code. Pick a target. It's running."

## Project Overview

Runway is a Tauri-based macOS app that lets developers deploy and manage code — especially AI-generated code from Claude Code, Codex, Cursor — to local machines, remote Linux servers, AWS Lambda, and Cloudflare Workers. It includes a lightweight Rust agent for remote execution and exposes an MCP server so AI coding agents can deploy and monitor programmatically.

## Current Status (March 2026)

Phases 1-3 complete. Phase 4 (cloud targets + polish) is next.

**Working end-to-end:** Project CRUD, runtime detection, agent-based Run/Stop with log streaming (local via UDS, remote via NATS), Paste & Deploy, xterm.js terminal, menu bar tray, cron scheduling.
**MCP server:** 17 tools (stdio transport), Claude Code verified end-to-end.
**CLI:** Full command set — list, deploy, run, stop, status, logs, import, detect, delete, health, schedule, targets.
**Remote agents:** Persistent agent with SQLite store (`~/.runway/agent.db`), 14 gRPC RPCs + NATS command channel, agent-side scheduler, store-and-forward events, remote upgrade capability. Deployed and tested on 192.168.1.222.
**NATS command channel:** All remote agent communication routed through NATS (Synadia Cloud). Neither desktop nor agent needs to expose ports. `AgentTransport` trait abstracts gRPC vs NATS — transport selected per target based on `nats_enabled` flag.
**Phase 4 progress:** macOS notifications on run complete/fail (manual + scheduled, local + remote, per-project toggle), in-app code editor (view/edit entrypoint with Cmd+S), target selector in Paste & Deploy, auto-run on create wired up, execution history, theme system with 3-way selector.
**Built but not wired:** mTLS certificate infrastructure (tls.rs), Keychain credential storage (credentials.rs).

### Persistent Remote Agent (March 7, 2026)
The remote agent (`runway-agent`) was redesigned from a stateless gRPC server to a production-grade persistent service:
- **AgentStore** (`agent_store.rs`): SQLite at `~/.runway/agent.db` with 5 tables (deployments, executions, execution_logs, events, schedules)
- **PersistentAgentService** (`persistent_service.rs`): 14 gRPC RPCs — Deploy, Execute, Stop, Health, Status, StreamLogs + 8 new RPCs (GetExecutions, GetExecutionLogs, GetEvents, AckEvents, AddSchedule, RemoveSchedule, ListSchedules, Upgrade)
- **Agent Scheduler** (`agent_scheduler.rs`): Independent tick() loop every 30s, runs cron jobs even when app is offline
- **Store-and-forward events**: Agent stores events locally, app polls via GetEvents RPC when connected
- **Remote upgrade**: Client-streaming Upgrade RPC — receive binary, verify, swap, restart via systemd
- **Dependency install**: Deploy RPC runs `pip install`, `npm install`, `go mod download` during deployment
- **AgentClient** (`agent_client.rs`): 8 new client methods + types (RemoteExecution, RemoteEvent, RemoteSchedule)
- **Deployment**: Built natively on remote server, runs as systemd service with auto-restart

### NATS Command Channel (March 7, 2026)
Remote agent communication fully routed through NATS — no direct network connection needed between desktop and agent:
- **AgentTransport trait** (`agent_transport.rs`): Unified async trait with health/status/stop/execute_streaming/deploy_streaming + schedule ops. Both `AgentClient` (gRPC) and `NatsAgentClient` (NATS) implement it.
- **NatsAgentClient** (`nats_cmd_client.rs`): Desktop-side NATS command client. Uses request-reply for simple RPCs, publish+subscribe for streaming (Execute, Deploy, Logs).
- **NatsCommandHandler** (`nats_cmd_handler.rs`): Agent-side NATS command subscriber. Listens on `runway.<agent_id>.cmd.>`, dispatches to `PersistentAgentService::do_*()` methods.
- **Transport selection**: `get_agent_client()` returns `Box<dyn AgentTransport>`. If target has `nats_enabled=true` + `nats_agent_id`, routes through NATS; otherwise falls back to gRPC.
- **Subject hierarchy**: `runway.<agent_id>.cmd.<type>` for commands, `runway.<agent_id>.resp.<request_id>` for streaming responses, plus existing event/log/heartbeat subjects via JetStream.
- **Target UI**: Add Target form includes optional "NATS Agent ID" field. Green "NATS" badge on enabled targets. `update_target_nats` Tauri command for toggling.
- **Zero inbound ports**: Both desktop and agent connect outbound to `tls://connect.ngs.global`. Works behind NAT, firewalls, different networks.

See `tasks.md` for detailed pending work. The app runs via `cargo tauri dev` on macOS.

## Design Philosophy

### Aesthetic is not decoration — it IS the product
- Tauri 2.0 with system WebKit. Native-feeling via macOS integrations (Keychain, notifications, menu bar) through Rust.
- Invest heavily in CSS polish: system fonts (`-apple-system`), vibrancy effects (`-webkit-backdrop-filter`), macOS-matching color tokens.
- Visual simplicity: every screen should have ONE primary action.
- Inspired by: Things 3 (task clarity), Tower (Git made visual), Linear (speed + beauty).
- Default to showing less. Progressive disclosure. Power users find depth; new users see simplicity.

### Simplicity as architecture
- One binary agent (Rust). No runtime dependencies. `curl | sh` install.
- One app. Not a suite, not a platform, not a "solution."
- Configuration is optional. Sensible defaults for everything.
- Zero accounts required for local use. Sign-up only when you need remote/sync.

### AI-controllable by design
- Runway exposes an MCP server. Claude Code (or any MCP client) can:
  - List projects and their status
  - Deploy code to any configured target
  - Read logs and monitoring data
  - Start/stop/restart deployments
  - Configure agents and targets
- CLI commands mirror every MCP tool (human and AI parity).
- The app, the CLI, and the MCP server share the same Rust core engine.

## Architecture

```
runway-app/          # Tauri 2.0 app (Rust backend + React frontend)
runway-agent/        # Rust binary (runs locally or on remote targets)
runway-cli/          # CLI interface (thin wrapper over core)
runway-core/         # Shared Rust library (business logic, gRPC, models)
runway-mcp/          # MCP server implementation (Rust)
```

### Communication
- Local agent: Unix domain socket or localhost gRPC
- Remote agent: NATS command channel (primary) — both sides connect outbound to Synadia Cloud, zero inbound ports
- Remote agent fallback: gRPC over TLS with mTLS client certificates (when NATS not configured)
- LAN discovery: mDNS via `zeroconf` crate
- MCP: stdio transport (Claude Code spawns it) or HTTP via `axum`

### Key Data Flow
```
Code Input -> Runtime Detection -> Target Selection -> Deploy -> Monitor
     |              |                    |               |          |
  paste/dir    python/node/go      local/vps/lambda   agent     logs/cpu/status
```

## Tech Stack

| Component | Technology | Why |
|-----------|-----------|-----|
| Desktop App | Tauri 2.0 + React 19 + TypeScript | Native feel via system WebKit, web dev velocity |
| UI Framework | shadcn/ui + Tailwind CSS | Rapid component assembly, macOS-matching aesthetic |
| Terminal/Logs | xterm.js | Proven terminal emulator, ANSI color support |
| Rust Backend | tokio + tonic + axum | Async runtime, gRPC, HTTP for MCP |
| Agent | Rust (same crate as backend) | Single binary, cross-platform, shared code with app |
| Communication | gRPC + protobuf (tonic) | Type-safe, streaming, mTLS support |
| MCP Server | Rust (axum for HTTP, tokio::io for stdio) | Embeds in app backend, no separate process |
| Local DB | SQLite via rusqlite | Cross-platform, no server needed |
| Credentials | security-framework crate (Mac) / encrypted file (agent) | Keychain access via Rust |

## Coding Standards

### TypeScript / React (Frontend)
- React 19 with functional components only. No class components.
- Strict TypeScript (`strict: true`). No `any` unless unavoidable with documented reason.
- TanStack Query for data fetching from Rust backend via Tauri `invoke`.
- Tailwind CSS for styling. No CSS-in-JS. Utility-first.
- Components in PascalCase files. One component per file.
- State management: React context for global state, local state with `useState`/`useReducer`.

### Rust (Backend + Agent + Core)
- Async everywhere via `tokio` runtime.
- `tonic` for gRPC client/server. `axum` for HTTP endpoints.
- `serde` + `serde_json` for all serialization.
- `clippy` with strict lints. Fix all warnings.
- No `unsafe` unless justified with a comment explaining why.
- Error handling: `thiserror` for library errors, `anyhow` for application errors.
- Structured logging via `tracing` crate.
- No global mutable state. Pass dependencies via structs.

### General
- No comments explaining obvious code. Comment only WHY, never WHAT.
- Error messages must be actionable: say what went wrong AND what to do about it.
- All user-facing strings must be localizable from day one.
- Tests for business logic. UI tested via Storybook or component tests. Integration tests for agent communication.

## MCP Server Specification

The MCP server is a CORE feature, not an add-on. It must be designed and shipped alongside the GUI.

### Tools exposed via MCP

```
runway_list_projects      # List all projects with status
runway_project_status     # Get detailed status of a project
runway_deploy             # Deploy code to a target
runway_stop               # Stop a running deployment
runway_restart            # Restart a deployment
runway_logs               # Stream or fetch logs
runway_list_targets       # List configured deploy targets (local, VPS, Lambda, etc.)
runway_add_target         # Configure a new deploy target
runway_list_agents        # List connected agents and their status
runway_import_code        # Import code from path or stdin
runway_detect_runtime     # Auto-detect language, deps, schedule needs
runway_health             # Overall system health check
```

### CLI mirrors MCP
```bash
runway list                    # same as runway_list_projects
runway deploy ./my-bot --target lambda-prod
runway logs my-crawler --follow
runway status my-bot
runway targets add my-vps --host 192.168.1.50 --key ~/.ssh/id_ed25519
runway agent install ubuntu@my-server.com
```

## Roadmap

### Phase 1: Foundation (Month 1-2)
**Goal: App shell + local execution**

- [x] Tauri app scaffold with React: project list, detail view, code import
- [x] runway-core (Rust): project model, runtime detection (Python, Node, Go, shell)
- [x] Local execution: Run/Stop via embedded agent (UDS), log streaming via events
- [x] Local agent communication: gRPC service via tonic on localhost:50051
- [x] "Paste & Deploy" flow: paste code → save to disk → detect runtime → run
- [x] xterm.js log viewer: ANSI color support, auto-fit, 10k scrollback
- [x] Basic monitoring: run count, last run time, exit codes, live uptime
- [x] Menu bar presence via Tauri tray plugin: quick status of running projects

**Ship:** Internal dogfood build. Use it yourself daily.

### Phase 2: MCP + CLI (Month 3)
**Goal: AI agents can control Runway**

- [x] MCP server (stdio transport): 13 tools — list, status, deploy, run, stop, logs, import, detect, delete, health, schedule_add, schedule_list, schedule_remove
- [x] CLI tool: `runway list|deploy|run|stop|status|logs|import|detect|delete|health|schedule`
- [x] .mcp.json config for easy Claude Code integration
- [x] Test: Claude Code can deploy a script via MCP end-to-end (verified: deploy inline code, run, delete)
- [x] Scheduling: cron-like local scheduling (@every, @hourly, @daily, @weekly, M H * * *) with CLI + MCP tools
- [x] Auto-detect improvements: parse requirements.txt, package.json (deps + scripts), go.mod, Cargo.toml

**Ship:** Beta via direct download. Invite 20 developers from AI coding communities.

### Phase 3: Remote Agents (Month 4-5)
**Goal: Deploy to your own Linux server**

- [x] Agent install script: `scripts/install-agent.sh` with systemd service, --uninstall flag
- [x] gRPC + mTLS: secure agent-to-app communication over internet (rcgen CA + server/client certs, tonic TLS config helpers)
- [x] Target management UI: add/edit/remove remote targets (React page + Tauri commands)
- [x] Remote deploy: gRPC client in runway-core, CLI deploy to remote targets, MCP tools
- [x] Agent health monitoring: real CPU/mem via sysinfo crate, ping command in CLI + UI
- [x] Credential management via security-framework (Keychain on Mac) — store/get/delete SSH keys and AWS credentials
- [x] mDNS discovery for LAN agents — mdns-sd crate, register/discover `_runway._tcp.local.` services
- [x] Target model: Target struct, SQLite table, store CRUD, 17 MCP tools total
- [x] CLI: `runway targets add|list|remove|ping` with real gRPC health checks

**Ship:** Beta expansion. Target 100 users.

### Phase 4: Cloud Targets + Polish (Month 5-6)
**Goal: AWS Lambda deploy + production readiness**

- [ ] AWS Lambda target: package code, create/update function, configure trigger
- [ ] AWS credential management (stored in Keychain, assumed roles)
- [ ] Cloudflare Workers target (stretch goal)
- [ ] Deployment history and rollback
- [x] Notification system: macOS notifications for failures, completions (manual + scheduled + remote via NATS)
- [ ] Settings, onboarding flow, empty states
- [ ] Anonymized telemetry: opt-in collection of crash reports, error frequency, and usage stats to improve code quality (no PII, transparent data policy)
- [ ] Code signing, notarization, DMG packaging, Homebrew cask

**Ship:** Public launch via direct download + Homebrew cask.

### Phase 5: Growth (Month 7+)
- [ ] Pro tier + subscription infrastructure
- [ ] Team features: shared projects, shared targets
- [ ] Web dashboard (companion, not replacement)
- [ ] Template library: common deployment patterns
- [ ] Community agent registry
- [ ] Windows/Linux builds (same Tauri codebase, platform-specific adaptations)
- [ ] App Store distribution (optional, if sandboxing requirements are met)

## MVP Definition (What ships publicly at Month 6)

### MUST have (launch blockers)
- Tauri macOS app with project list and detail views
- Paste & Deploy: paste code or select directory -> run locally
- Runtime detection: Python, Node, Go, Rust, shell scripts
- Local scheduling (cron-like)
- Remote agent deploy to Linux VPS
- AWS Lambda deploy (single function)
- MCP server with core tools (deploy, status, logs, list)
- CLI with matching commands
- xterm.js log viewer with streaming output
- Menu bar quick access via system tray

### SHOULD have (important but won't block launch)
- Cloudflare Workers target
- Deployment history/rollback
- mDNS LAN discovery
- Agent auto-update mechanism
- Anonymized telemetry for crash reports and error tracking (opt-in, no PII)

### WON'T have at launch
- Team/collaboration features
- Web dashboard
- Windows/Linux builds
- Managed hosting
- Template marketplace
- User accounts for local-only use

## File Naming Conventions
- TypeScript components: PascalCase (`ProjectList.tsx`, `DeployWizard.tsx`)
- TypeScript utilities: camelCase (`runtimeDetect.ts`, `invokeBackend.ts`)
- Rust files: snake_case (`agent_server.rs`, `runtime_detect.rs`)
- Proto files: snake_case (`runway_service.proto`)
- Config files: lowercase with dots (`runway.config.json`)

## Git Conventions
- Branch format: `feat/short-description`, `fix/short-description`
- Commit messages: imperative mood, max 72 chars first line
- One logical change per commit

## Key Decisions Log
| Decision | Choice | Rationale |
|----------|--------|-----------|
| App framework | Tauri 2.0 (React + Rust) | Cross-platform path, Rust gRPC/MCP story, market narrative |
| Framework debate | Tauri over SwiftUI | Decided via 3-round AI debate: technical + market perspectives won 2-1 over UX-only argument |
| Agent language | Rust (shared with app backend) | Single language for backend + agent, shared crate, solo founder simplification |
| Frontend | React 19 + TypeScript + shadcn/ui | Largest talent pool, npm ecosystem, hot reload, xterm.js for logs |
| Communication | gRPC + mTLS (tonic) | Type safety, streaming, mutual auth, Rust-native |
| MCP transport | stdio (primary) + HTTP (axum) | Claude Code native; HTTP for remote/web clients |
| Local DB | SQLite (rusqlite) | Cross-platform, no server, shared between app and agent |
| Min macOS | 13 (Ventura) | Tauri 2.0 minimum; wider user base than macOS 15 |
| Distribution | Direct download + Homebrew cask (primary) | Developer-native distribution; App Store optional later |
| Pricing model | Freemium | Free local use, Pro for remote + cloud targets |
| Backend storage | SQLite (rusqlite) | 3-way debate: SQLite won 3-0 over YAML/JSON. AI agents get JSON via MCP, not flat files (DRF 006) |
| Execution architecture | Agent-only (no direct executor) | 4-round 3-way debate: Agent-Only won 2-1. Unified local+remote through AgentClient→AgentService. UDS for local, TCP gRPC for remote (DRF 007) |
| Remote transport | NATS command channel (primary) | Zero inbound ports, works behind NAT. AgentTransport trait unifies gRPC and NATS. gRPC fallback for targets without NATS |
