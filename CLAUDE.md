# Berth - Project Instructions

> Mac-native deployment control plane for AI-generated code.
> "Paste code. Pick a target. It's running."

## Project Overview

Berth is a Tauri-based macOS app that lets developers deploy and manage code — especially AI-generated code from Claude Code, Codex, Cursor — to local machines, remote Linux servers, AWS Lambda, and Cloudflare Workers. It includes a lightweight Rust agent for remote execution and exposes an MCP server so AI coding agents can deploy and monitor programmatically.

## Current Status (March 2026)

Phases 1-3 complete. Phase 4 (real workloads + polish) in progress. Strategic pivot: cloud targets (Lambda, Workers) deferred to Phase 5 — focusing instead on features that make Berth handle real projects: env vars, service mode, Docker Compose, public URL publishing.

**Working end-to-end:** Project CRUD, runtime detection, agent-based Run/Stop with log streaming (local via UDS, remote via NATS), Paste & Deploy, xterm.js terminal, menu bar tray, cron scheduling.
**MCP server:** 23 tools (stdio transport), Claude Code verified end-to-end. Includes publish/unpublish and env var management.
**CLI:** Full command set — list, deploy, run, stop, status, logs, import, detect, delete, health, schedule, targets, publish, unpublish, env, store (list/search/install).
**Remote agents:** Persistent agent with SQLite store (`~/.berth/agent.db`), 16 gRPC RPCs (incl. Publish/Unpublish) + NATS command channel, agent-side scheduler, store-and-forward events, remote upgrade capability. Deployed and tested on 192.168.1.222.
**NATS command channel:** All remote agent communication routed through NATS (Synadia Cloud). Neither desktop nor agent needs to expose ports. `AgentTransport` trait abstracts gRPC vs NATS — transport selected per target based on `nats_enabled` flag. All commands HMAC-signed with shared secret established during pairing.
**Agent distribution:** CI cross-compiles for x86_64 and aarch64 Linux via GitHub Actions (`release-agent.yml`). Tag `agent-v*` triggers build → publishes binaries + SHA256 checksums to `berth-app/berth-agent` GitHub releases. Install via `curl -sSL https://agent.getberth.dev/install.sh | sudo bash`. Agent self-upgrades by downloading binary from GitHub releases.
**Agent self-upgrade:** Cloudflared-model upgrade tested end-to-end (v0.3.1 → v0.4.0). Agent downloads binary from GitHub, verifies SHA256, atomic swap, exit(42), systemd restarts with new binary. Rollback on failed probation.
**Security hardening (v0.4.0):** HMAC-SHA256 command signing with nonce replay prevention + 60s timestamp window, path traversal prevention (project names, entrypoints, filenames, tar archives, deploy dirs), 8-char pairing codes with challenge-response + rate limiting, shared secret establishment during pairing (agent SQLite + desktop Keychain), pre-pairing command rejection, randomized agent_id, CA key permission verification.
**Phase 4 progress:** macOS notifications on run complete/fail (manual + scheduled, local + remote, per-project toggle), in-app code editor (view/edit entrypoint with Cmd+S), target selector in Paste & Deploy, auto-run on create wired up, execution history, theme system with 3-way selector, **public URL publishing via cloudflared tunnels** (tested end-to-end March 9, 2026), **per-project environment variables** (store + UI + MCP + CLI + .env import + log masking, March 9, 2026), **template store** (CLI list/search/install, TemplateStore UI page, template tracking).
**Auth model designed (DRF-012):** Supabase auth (magic links, Raycast model), Stripe subscriptions, progressive identity (Anonymous → Free → Pro → Team). No code yet — design document only.
**Built and wired:** mTLS certificate infrastructure (tls.rs) with CA key permission verification, Keychain credential storage (credentials.rs) used for agent shared secrets.

### Persistent Remote Agent (March 7, 2026)
The remote agent (`berth-agent`) was redesigned from a stateless gRPC server to a production-grade persistent service:
- **AgentStore** (`agent_store.rs`): SQLite at `~/.berth/agent.db` with 5 tables (deployments, executions, execution_logs, events, schedules)
- **PersistentAgentService** (`persistent_service.rs`): 16 gRPC RPCs — Deploy, Execute, Stop, Health, Status, StreamLogs + 10 RPCs (GetExecutions, GetExecutionLogs, GetEvents, AckEvents, AddSchedule, RemoveSchedule, ListSchedules, Upgrade, Publish, Unpublish). Includes TunnelManager for public URL publishing.
- **Agent Scheduler** (`agent_scheduler.rs`): Independent tick() loop every 30s, runs cron jobs even when app is offline
- **Store-and-forward events**: Agent stores events locally, app polls via GetEvents RPC when connected
- **Remote upgrade**: Client-streaming Upgrade RPC — receive binary, verify, swap, restart via systemd
- **Dependency install**: Deploy RPC runs `pip install`, `npm install`, `go mod download` during deployment
- **AgentClient** (`agent_client.rs`): 8 new client methods + types (RemoteExecution, RemoteEvent, RemoteSchedule)
- **Deployment**: Cross-compiled via CI (GitHub Actions + `cross`), distributed as prebuilt binaries via GitHub releases (`berth-app/berth-agent`). Runs as systemd service with auto-restart

### NATS Command Channel (March 7, 2026)
Remote agent communication fully routed through NATS — no direct network connection needed between desktop and agent:
- **AgentTransport trait** (`agent_transport.rs`): Unified async trait with health/status/stop/execute_streaming/deploy_streaming + schedule ops. Both `AgentClient` (gRPC) and `NatsAgentClient` (NATS) implement it.
- **NatsAgentClient** (`nats_cmd_client.rs`): Desktop-side NATS command client. Uses request-reply for simple RPCs, publish+subscribe for streaming (Execute, Deploy, Logs).
- **NatsCommandHandler** (`nats_cmd_handler.rs`): Agent-side NATS command subscriber. Listens on `berth.<owner_id>.<agent_id>.cmd.>`, verifies HMAC signature before dispatching to `PersistentAgentService::do_*()` methods.
- **Transport selection**: `get_agent_client()` returns `Box<dyn AgentTransport>`. If target has `nats_enabled=true` + `nats_agent_id`, routes through NATS; otherwise falls back to gRPC.
- **Subject hierarchy**: `berth.<agent_id>.cmd.<type>` for commands, `berth.<agent_id>.resp.<request_id>` for streaming responses, plus existing event/log/heartbeat subjects via JetStream.
- **Target UI**: Add Target form includes optional "NATS Agent ID" field. Green "NATS" badge on enabled targets. `update_target_nats` Tauri command for toggling.
- **Zero inbound ports**: Both desktop and agent connect outbound to `tls://connect.ngs.global`. Works behind NAT, firewalls, different networks.

### Authentication Model (March 8, 2026) — DRF-012
Auth model designed but not yet implemented. Key decisions:
- **Provider:** Supabase (GoTrue auth + PostgreSQL + Edge Functions). 50K MAU free.
- **Login method:** Magic links only at launch (Raycast model). No passwords, no OAuth. GitHub OAuth added post-launch.
- **Identity lifecycle:** Anonymous (download) → Free (magic link signup, settings sync) → Pro ($12/mo, NATS relay + cloud) → Team ($25/user/mo, unlimited)
- **Auth state:** Tokens in macOS Keychain (never SQLite). Cached tier in SQLite settings table.
- **Tier enforcement:** Dual layer — client-side for UX gating, server-side (Supabase RLS + Edge Functions) for security. NATS creds provisioned server-side only for Pro+. MCP has NO tier checks.
- **Settings sync:** LWW merge for theme, default_target, auto_run, log_scrollback. Local-only: nats_url, nats_creds, github_token, install_id.
- **Subscription:** Stripe Checkout + Customer Portal. 7-day offline grace period.
- **Desktop flow:** Email input in Settings modal → Supabase magic link → `auth.berth.sh/callback` bridge page → `berth://auth/callback` deep link → Tauri handles token exchange.
- **Implementation:** Phase 5 — `auth.rs`, `supabase.rs` in berth-core, `AccountSection.tsx` + `AuthModal.tsx` in React, Supabase Edge Functions for profile/sync/checkout/nats-provisioning.

### Security Hardening (March 9, 2026) — v0.4.0
Full security audit and remediation of remote agent communication. Three phases implemented:

**P0 — HMAC Message Signing + Path Traversal:**
- **HMAC-SHA256 command signing** (`message_auth.rs`): All NATS commands carry signature, nonce, timestamp. Agent verifies before dispatching. Desktop signs on send. 60s timestamp freshness window, bounded nonce tracker (10K entries) prevents replay.
- **Path safety** (`path_safety.rs`): `sanitize_project_name()`, `sanitize_entrypoint()`, `sanitize_filename()`, `validate_path_within()`. Applied in Tauri commands, MCP tools, agent deploy/execute paths. Rejects `../`, absolute paths, null bytes, control chars.
- **Tar archive validation**: Every entry validated on extraction — rejects `../` components, absolute paths, suspicious symlinks/hardlinks.
- **Upload token auth**: Per-upload random UUID token generated on deploy start, required on every chunk.
- **Deploy path validation**: `deploy_dir()` rejects project_ids with `../` or path separators. `do_execute()` validates entrypoint is relative and working_dir is within deploys/tmp.

**P1 — Pairing Hardening:**
- **8-char codes** (32^8 ≈ 1.1T combinations, was 6-char/32^6 ≈ 1B). 5-minute expiry (was 15 min).
- **Challenge-response**: Agent generates random UUID challenge in advertisement. Desktop computes `HMAC-SHA256(challenge, pairing_code)` and sends in claim. Agent verifies.
- **Rate limiting**: 5 failed claims → 5min cooldown. 3 expired codes without success → requires agent restart.
- **Shared secret establishment**: Agent generates 256-bit random secret on successful pairing. Stored in agent SQLite (`agent_config` table) and sent in `PairingAck`. Desktop stores in macOS Keychain as `agent-secret:{target_id}`.
- **Pre-pairing command rejection**: Agent rejects ALL NATS commands when no shared secret is configured (was accepting all).
- **File permissions**: `agent.env` set to 0o600 on Unix after writing `BERTH_OWNER_ID`.

**P2 — Identity Hardening:**
- **Randomized agent_id**: First start generates `{hostname}-{uuid8}`, persisted in SQLite. CLI `--nats-agent-id` still overrides.
- **Removed owner_id fallback**: `get_agent_client()` no longer falls back to `install_id`. Targets without `owner_id` must re-pair.
- **CA key permission check**: `load_ca()` verifies key file has 0o600 permissions, auto-fixes with warning if drifted.

**Pending security items** (documented in `.claude/projects/.../memory/security.md`):
- Executor `expect()` → `ok_or()` (medium)
- NATS command rate limiting post-pairing (medium)
- Nonce tracker persistence to SQLite (medium)
- NATS ACLs, per-agent credentials, binary signature verification (needs infrastructure)

### Agent Distribution (March 9, 2026) — CI Cross-Compilation
- **CI workflow**: `.github/workflows/release-agent.yml` triggers on `agent-v*` tags
- **Cross-compilation**: `cross` crate builds for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu` on Ubuntu runners
- **Artifacts**: Binary + SHA256 checksum per architecture, uploaded as GitHub release assets to `berth-app/berth-agent` public repo
- **Install**: `curl -sSL https://agent.getberth.dev/install.sh | sudo bash`
- **Self-upgrade**: Agent downloads new binary from GitHub releases, verifies SHA256, atomic swap, exit(42), systemd restarts

### Agent Self-Upgrade (March 8, 2026) — Cloudflared Model
Tested end-to-end: v0.3.1 → v0.4.0 fully automated, zero manual intervention.
- **Pattern:** Agent downloads binary from GitHub, verifies SHA256, atomic rename swap, exit(42), systemd restarts with new binary
- **Key systemd settings:** `SuccessExitStatus=42` (prevents rate-limiting), `ExecStopPost=+/usr/local/lib/berth/rollback.sh` (runs as root)
- **Probation:** 30s window, 3 TCP self-connects required to pass. Rollback on failure.
- **CLI:** `berth-agent update [--version X.Y.Z] [--yes]` for self-serve updates
- **Systemd service**: Runs as user `berth` at `/home/berth/.berth/bin/berth-agent`, config in `/home/berth/.berth/agent.env`

### Public URL Publishing (March 9, 2026) — Cloudflared Tunnels
Tested end-to-end: run a Python HTTP server, click Publish, get a public trycloudflare.com URL.
- **Architecture:** Pluggable `TunnelProvider` enum in `tunnel.rs`. Only cloudflared implemented initially. Adding a new provider (ngrok, bore, custom) requires changes to ONE file only.
- **TunnelManager:** Spawns cloudflared, parses URL from stderr (30s timeout), keeps stderr drained to prevent SIGPIPE. Stores active tunnels in memory. Stop project = stop tunnel.
- **Full stack:** Proto RPCs → AgentTransport trait → gRPC client/NATS client → Agent service (local + remote) → NATS command handler → Tauri commands → React PublishPanel → MCP tools → CLI commands
- **SQLite:** `tunnel_url` and `tunnel_provider` columns on `projects` table. `set_tunnel_url()`, `clear_tunnel_url()` store methods.
- **UI:** PublishPanel component in ProjectDetail toolbar. Port input + Publish button when running. Green URL bar with copy + Unpublish when published.
- **MCP:** `berth_publish(project_id, port, provider?)`, `berth_unpublish(project_id)`
- **CLI:** `berth publish <project> --port 8080 [--provider cloudflared]`, `berth unpublish <project>`
- **Bug fixes during implementation:** (1) Local agent had stub publish — now real TunnelManager. (2) cloudflared SIGPIPE — stderr drain task. (3) Stop returning error when process already exited — now resets status to idle.

### Per-Project Environment Variables (March 9, 2026)
Full-stack implementation: store env vars per project, pass to process at runtime, mask values in logs.
- **Core module** (`env.rs`): `parse_dotenv()` for .env file parsing (comments, quotes, `export` prefix), `mask_env_values()` for log masking (values ≥ 3 chars replaced with `***`, longest-first).
- **Store layer**: `project_env_vars` table already existed. `set_env_var()`, `get_env_vars()`, `delete_env_var()` methods in `store.rs`.
- **Execution wiring**: `run_project()` in commands.rs loads env vars from store, passes via `ExecuteParams`, masks values in log events and execution history before storage.
- **UI**: `EnvVarsPanel` component in ProjectDetail side panel (Key icon in toolbar). Key/value rows with eye-toggle reveal, delete button, add form, .env import textarea.
- **MCP**: 4 tools — `berth_env_set(project_id, key, value)`, `berth_env_get(project_id)`, `berth_env_delete(project_id, key)`, `berth_env_import(project_id, content)`. Also wired env vars into `handle_run` and `handle_deploy`.
- **CLI**: `berth env set|list|remove|import` subcommand. Also wired env vars into `berth run`, `berth deploy`, `berth logs`.
- **Design decisions**: Desktop-side storage only (env vars never persisted on remote agent). All values treated as secrets (no "secret" flag). .env import merges (upsert).

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
- Berth exposes an MCP server. Claude Code (or any MCP client) can:
  - List projects and their status
  - Deploy code to any configured target
  - Read logs and monitoring data
  - Start/stop/restart deployments
  - Configure agents and targets
- CLI commands mirror every MCP tool (human and AI parity).
- The app, the CLI, and the MCP server share the same Rust core engine.

## Architecture

```
berth-app/          # Tauri 2.0 app (Rust backend + React frontend)
berth-agent/        # Rust binary (cross-compiled for Linux x86_64/aarch64, distributed via GitHub releases)
berth-cli/          # CLI interface (thin wrapper over core)
berth-core/         # Shared Rust library (business logic, gRPC, models)
berth-proto/        # Shared contract crate (proto types, NATS relay, message auth, runtime types)
berth-mcp/          # MCP server implementation (Rust)
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
berth_list_projects      # List all projects with status
berth_project_status     # Get detailed status of a project
berth_deploy             # Deploy code to a target
berth_stop               # Stop a running deployment
berth_restart            # Restart a deployment
berth_logs               # Stream or fetch logs
berth_list_targets       # List configured deploy targets (local, VPS, Lambda, etc.)
berth_add_target         # Configure a new deploy target
berth_list_agents        # List connected agents and their status
berth_import_code        # Import code from path or stdin
berth_detect_runtime     # Auto-detect language, deps, schedule needs
berth_health             # Overall system health check
berth_publish            # Publish a running project to a public URL via tunnel
berth_unpublish          # Stop the public URL tunnel for a project
berth_env_set            # Set an environment variable for a project
berth_env_get            # Get all environment variables for a project
berth_env_delete         # Delete an environment variable from a project
berth_env_import         # Import variables from .env format
```

### CLI mirrors MCP
```bash
berth list                    # same as berth_list_projects
berth deploy ./my-bot --target lambda-prod
berth logs my-crawler --follow
berth status my-bot
berth targets add my-vps --host 192.168.1.50 --key ~/.ssh/id_ed25519
berth publish my-api --port 8080       # Public URL via cloudflared
berth unpublish my-api
berth env set my-bot API_KEY sk-123    # Set env var
berth env list my-bot                  # List env vars (values masked)
berth env remove my-bot API_KEY        # Remove env var
berth env import my-bot .env           # Import from .env file
```

## Roadmap

### Phase 1: Foundation (Month 1-2)
**Goal: App shell + local execution**

- [x] Tauri app scaffold with React: project list, detail view, code import
- [x] berth-core (Rust): project model, runtime detection (Python, Node, Go, shell)
- [x] Local execution: Run/Stop via embedded agent (UDS), log streaming via events
- [x] Local agent communication: gRPC service via tonic on localhost:50051
- [x] "Paste & Deploy" flow: paste code → save to disk → detect runtime → run
- [x] xterm.js log viewer: ANSI color support, auto-fit, 10k scrollback
- [x] Basic monitoring: run count, last run time, exit codes, live uptime
- [x] Menu bar presence via Tauri tray plugin: quick status of running projects

**Ship:** Internal dogfood build. Use it yourself daily.

### Phase 2: MCP + CLI (Month 3)
**Goal: AI agents can control Berth**

- [x] MCP server (stdio transport): 13 tools — list, status, deploy, run, stop, logs, import, detect, delete, health, schedule_add, schedule_list, schedule_remove
- [x] CLI tool: `berth list|deploy|run|stop|status|logs|import|detect|delete|health|schedule`
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
- [x] Remote deploy: gRPC client in berth-core, CLI deploy to remote targets, MCP tools
- [x] Agent health monitoring: real CPU/mem via sysinfo crate, ping command in CLI + UI
- [x] Credential management via security-framework (Keychain on Mac) — store/get/delete SSH keys and AWS credentials
- [x] mDNS discovery for LAN agents — mdns-sd crate, register/discover `_berth._tcp.local.` services
- [x] Target model: Target struct, SQLite table, store CRUD, 23 MCP tools total
- [x] CLI: `berth targets add|list|remove|ping` with real gRPC health checks

**Ship:** Beta expansion. Target 100 users.

### Phase 4: Real Workloads + Polish (Month 5-6)
**Goal: Handle real projects, not just scripts**

- [x] Per-project environment variables: SQLite store + EnvVarsPanel UI (key/value editor with reveal toggle, .env import) + 4 MCP tools (`berth_env_set/get/delete/import`) + CLI `berth env set|list|remove|import` + log masking of secret values. Env vars loaded from store and passed to agent at execute time. March 9, 2026.
- [x] Service mode (keep running): `run_mode: oneshot | service` on Project, auto-restart on crash with exponential backoff (1s→60s cap), uptime tracking, restart count, port config for web services, supervisor loop in agent. March 9, 2026.
- [ ] Docker-Compose support: detect docker-compose.yml/compose.yaml in runtime detection, new `DockerCompose` runtime variant, `compose.rs` module (up/down/ps/logs via podman-compose or docker-compose), transparent through existing deploy/run/stop commands
- [x] Public URL publishing (pluggable providers): "Publish" button on running projects. Berth orchestrates, user picks tunnel provider — Cloudflare Quick Tunnel (free, no account). Publish/Unpublish gRPC RPCs, MCP `berth_publish`/`berth_unpublish` tools, CLI `berth publish/unpublish`. Zero Berth-side infrastructure or liability. Tested end-to-end March 9, 2026.
- [x] Notification system: macOS notifications for failures, completions (manual + scheduled + remote via NATS)
- [ ] Code signing, notarization, DMG packaging, Homebrew cask
- [ ] Settings, onboarding flow, empty states

**Ship:** Public launch via direct download + Homebrew cask.

### Phase 5: Cloud Targets + Growth (Month 7+)
- [ ] AWS Lambda target: package code, create/update function, configure trigger
- [ ] AWS credential management (stored in Keychain, assumed roles)
- [ ] Cloudflare Workers target
- [ ] Auth implementation: Supabase magic links, AccountSection in Settings, deep link handler (DRF-012)
- [ ] Stripe subscription: Pro $12/mo, Team $25/user/mo, Checkout + Customer Portal
- [ ] Settings sync: cloud backup via Supabase (synced on login, debounced on change)
- [ ] NATS credential provisioning: server-side via Supabase Edge Function (Pro+ only)
- [ ] Team features: shared projects, shared targets
- [ ] Web dashboard (companion, not replacement)
- [ ] Template library: common deployment patterns
- [ ] Community agent registry
- [ ] Windows/Linux builds (same Tauri codebase, platform-specific adaptations)
- [ ] App Store distribution (optional, if sandboxing requirements are met)
- [ ] Deployment history and rollback
- [ ] Anonymized telemetry: opt-in collection of crash reports, error frequency, and usage stats (no PII)

## MVP Definition (What ships publicly at Month 6)

### MUST have (launch blockers)
- Tauri macOS app with project list and detail views
- Paste & Deploy: paste code or select directory -> run locally
- Runtime detection: Python, Node, Go, Rust, shell scripts, Docker Compose
- Per-project environment variables (stored, masked in logs)
- Service mode: keep-running with auto-restart for API servers, bots, workers
- Local scheduling (cron-like)
- Remote agent deploy to Linux VPS
- Public URL publishing via pluggable tunnel providers (cloudflared/ngrok/custom — click Publish → get a URL)
- MCP server with core tools (deploy, status, logs, list, env, publish)
- CLI with matching commands
- xterm.js log viewer with streaming output
- Menu bar quick access via system tray

### SHOULD have (important but won't block launch)
- Custom domain publishing (users bring their own Cloudflare/ngrok account)
- Deployment history/rollback
- mDNS LAN discovery
- Agent auto-update mechanism
- Code signing, notarization, DMG, Homebrew cask

### WON'T have at launch
- AWS Lambda / Cloudflare Workers targets
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
- Proto files: snake_case (`berth_service.proto`)
- Config files: lowercase with dots (`berth.config.json`)

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
| Pricing model | Freemium (transport-gated) | Free = direct gRPC (zero cost). Pro = NATS relay + cloud targets. MCP free on all tiers (DRF 008) |
| Backend storage | SQLite (rusqlite) | 3-way debate: SQLite won 3-0 over YAML/JSON. AI agents get JSON via MCP, not flat files (DRF 006) |
| Execution architecture | Agent-only (no direct executor) | 4-round 3-way debate: Agent-Only won 2-1. Unified local+remote through AgentClient→AgentService. UDS for local, TCP gRPC for remote (DRF 007) |
| Remote transport | NATS command channel (primary) | Zero inbound ports, works behind NAT. AgentTransport trait unifies gRPC and NATS. gRPC fallback for targets without NATS |
| Auth provider | Supabase (GoTrue + PostgreSQL + Edge Functions) | 50K MAU free, auth+DB+edge functions in one, REST API via reqwest (DRF 012) |
| Auth method | Magic links only (Raycast model) | No passwords, no OAuth at launch. Simplest possible auth surface. GitHub OAuth added post-launch (DRF 012) |
| Subscription | Stripe Checkout + Customer Portal | No PCI scope. Webhooks handled by Supabase Edge Functions (DRF 012) |
| Agent self-upgrade | Cloudflared model — exit(42) + systemd restart | Atomic binary swap, 30s probation, automatic rollback on failure |
| Phase 4 pivot | Real workloads over cloud targets | Cloud targets (Lambda, Workers) deferred to Phase 5. Env vars, service mode, Compose, public URLs increase value for existing local+remote story. 3.4:1 infra-to-feature ratio indicated over-engineering — focus on user value |
| Public URL publishing | Pluggable tunnel providers (cloudflared/ngrok/custom) | Berth orchestrates, user picks provider. Zero Berth infrastructure or liability. No relay server to host. Users who want custom domains bring their own Cloudflare/ngrok account |
| Agent distribution | CI cross-compilation + GitHub releases | `cross` crate builds x86_64 + aarch64 Linux on GitHub Actions. Published to `berth-app/berth-agent`. Agent self-upgrades from releases |
| NATS command security | HMAC-SHA256 signing + challenge-response pairing | Shared secret established during pairing (agent SQLite + desktop Keychain). All commands signed with nonce + timestamp. Pre-pairing commands rejected |
| Pairing codes | 8-char codes (32^8 ≈ 1.1T) + 5min expiry | Challenge-response (HMAC of challenge using code as key). Rate limiting (5 failures → cooldown). Replaces 6-char/15min |
