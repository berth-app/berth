# Runway - Project Instructions

> Mac-native deployment control plane for AI-generated code.
> "Paste code. Pick a target. It's running."

## Project Overview

Runway is a Tauri-based macOS app that lets developers deploy and manage code — especially AI-generated code from Claude Code, Codex, Cursor — to local machines, remote Linux servers, AWS Lambda, and Cloudflare Workers. It includes a lightweight Rust agent for remote execution and exposes an MCP server so AI coding agents can deploy and monitor programmatically.

## Current Status (March 2026)

Phase 1 scaffold is complete. The app compiles, launches, and renders.

**Working end-to-end:** Create project (SQLite), list projects, detect runtime, delete project.
**UI exists but not connected:** Run/Stop buttons, log viewer, paste-code-to-disk flow.
**Stubs only:** MCP server, CLI, remote agent, gRPC communication.

The app runs via `cargo tauri dev` on macOS. Private GitHub repo created.

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
- Remote agent: gRPC over TLS with mTLS client certificates
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
- [x] Local execution: Run/Stop via Tauri commands, ProcessRegistry, log streaming via events
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
- [ ] gRPC + mTLS: secure agent-to-app communication over internet (TLS not yet implemented)
- [x] Target management UI: add/edit/remove remote targets (React page + Tauri commands)
- [x] Remote deploy: gRPC client in runway-core, CLI deploy to remote targets, MCP tools
- [x] Agent health monitoring: real CPU/mem via sysinfo crate, ping command in CLI + UI
- [ ] Credential management via security-framework (Keychain on Mac)
- [ ] mDNS discovery for LAN agents
- [x] Target model: Target struct, SQLite table, store CRUD, 17 MCP tools total
- [x] CLI: `runway targets add|list|remove|ping` with real gRPC health checks

**Ship:** Beta expansion. Target 100 users.

### Phase 4: Cloud Targets + Polish (Month 5-6)
**Goal: AWS Lambda deploy + production readiness**

- [ ] AWS Lambda target: package code, create/update function, configure trigger
- [ ] AWS credential management (stored in Keychain, assumed roles)
- [ ] Cloudflare Workers target (stretch goal)
- [ ] Deployment history and rollback
- [ ] Notification system: macOS notifications for failures, completions
- [ ] Settings, onboarding flow, empty states
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
