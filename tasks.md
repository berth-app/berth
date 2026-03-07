# Runway - Pending Tasks

> Comprehensive task tracker. Synced with CLAUDE.md roadmap.

## Legend

- `[ ]` — Not started
- `[~]` — In progress / partially done
- `[x]` — Complete
- Priority: **P0** (blocker), **P1** (important), **P2** (nice-to-have)

---

## Bugs & Fixes

- [x] ~~**P0** Targets.tsx uses `addToast`~~ — verified correct: uses `const { toast } = useToast()` (false alarm)
- [x] **P1** CLAUDE.md "Current Status" section (line 10-16) is outdated — says "stubs only" for MCP/CLI/agent but all are complete through Phase 3
- [ ] **P2** `docs/remote-agent.html` created but not committed
- [x] **P1** `auto_run_on_create` setting stored in SQLite but never checked in create_project flow

---

## Security: Remote Agent Authentication

The TLS infrastructure exists (`tls.rs`) and Keychain storage exists (`credentials.rs`) but neither is wired into actual connections.

### mTLS Connection Security
- [ ] **P0** Agent: add `--tls` flag to load server cert and enable `ServerTlsConfig`
- [ ] **P0** Agent: require client certs signed by Runway CA when TLS is enabled
- [ ] **P0** Tauri `run_project` command (remote target path): use `client_tls_config()` when connecting to TLS-enabled agents
- [ ] **P1** Target registration: distribute server cert to agent during `runway agent install`
- [ ] **P1** Store CA private key in macOS Keychain instead of plaintext on disk
- [ ] **P2** Cert rotation / expiry management

### Agent Authentication & Authorization
- [ ] **P1** Shared secret / API key auth: agent requires token on gRPC metadata, app sends it
- [ ] **P1** UI: per-target auth token field in target add/edit form
- [ ] **P1** Store agent auth tokens in macOS Keychain via `credentials.rs`
- [ ] **P1** CLI: `runway targets add --auth-token <token>` flag
- [ ] **P2** Agent: `--generate-token` flag to create and display a random auth token on first run
- [ ] **P2** Agent: rate limiting on unauthenticated requests
- [ ] **P2** SSH key-based agent auth as alternative to token-based

---

## Phase 3: Remaining Items

Phase 3 is marked complete in CLAUDE.md but has loose ends:

- [x] **P1** PasteAndDeploy.tsx: add optional target selector for "deploy to remote" flow
- [ ] **P1** Remote execution: handle multi-file projects (currently sends single entrypoint file only)
- [ ] **P2** Agent auto-reconnect / retry logic in Tauri commands
- [ ] **P2** Remote execution: show target name in Terminal header when running remotely

---

## Architecture: Agent-Only Refactor (Complete)

- [x] Move AgentServiceImpl to runway-core (shared across all crates)
- [x] UDS transport helpers (`uds.rs` — serve + connect)
- [x] Local agent bootstrap with lockfile coordination (`local_agent.rs`)
- [x] Proto: exit_code + is_final fields on ExecuteResponse
- [x] Tauri: unified run_project/stop_project with optional target param
- [x] CLI: --target flag, AgentClient-based execution
- [x] MCP: AgentClient-based deploy/run/stop
- [x] Scheduler: AgentClient-based execution
- [x] Frontend: unified invoke calls (no more run_project_remote/stop_project_remote)

---

## Phase 4: Cloud Targets + Polish (Month 5-6)

### AWS Lambda Target
- [ ] **P0** Lambda target type: package code into ZIP, create/update function via AWS SDK
- [ ] **P0** Lambda runtime mapping: Python -> python3.12, Node -> nodejs20.x, etc.
- [ ] **P0** Lambda trigger configuration: API Gateway, CloudWatch Events, S3
- [ ] **P1** AWS credential management: store access keys in Keychain, support assumed roles
- [ ] **P1** Lambda deployment logs: stream CloudWatch logs back to Terminal
- [ ] **P2** Lambda cold start monitoring and cost estimation

### Cloudflare Workers Target (stretch)
- [ ] **P2** Workers target type: wrangler-compatible deploy
- [ ] **P2** Workers KV / D1 binding configuration

### Deployment History
- [x] **P1** Execution logs table in SQLite: project_id, started_at, finished_at, exit_code, output, trigger
- [x] **P1** UI: execution history list in ProjectDetail with expandable output
- [ ] **P1** Re-run from execution history: replay button on past execution rows
- [ ] **P2** Rollback: re-deploy a previous version

### Notifications
- [x] **P1** macOS notifications on manual run complete/fail via `tauri-plugin-notification`
- [x] **P1** macOS notifications on scheduled run complete/fail (local scheduler + NATS `schedule_triggered` + `execution_completed`)
- [ ] **P1** macOS notification on agent offline (during ping or failed connection)
- [x] **P2** Notification preferences per project (toggle in ProjectDetail)
- [ ] **P2** Notification sound customization

### Code Editing
- [x] **P1** In-app code editor: view and edit project entrypoint file from ProjectDetail
- [ ] **P1** Syntax highlighting (CodeMirror or Monaco) for Python, JS/TS, Go, Rust, Shell
- [ ] **P2** Multi-file project browser: tree view of project directory
- [ ] **P2** Save and re-run: edit code → save → auto-run workflow

### Themes
- [x] **P1** Theme system: CSS custom properties with `data-theme` attribute override
- [x] **P1** Three-way theme selector in Settings (System / Dark / Light)
- [ ] **P1** Custom accent color picker in Settings
- [ ] **P2** Additional theme presets (Solarized, Nord, Monokai)
- [ ] **P2** Vibrancy / translucency effects via `-webkit-backdrop-filter` on macOS

### Settings & Onboarding
- [x] **P1** Settings page: default target, theme, log scrollback, auto-run on create
- [ ] **P1** Settings: notification preferences section
- [ ] **P1** Settings: certificate management UI (view CA, regenerate certs)
- [ ] **P1** Onboarding flow: first-launch wizard (create first project or paste code)
- [ ] **P1** Empty states: better zero-state for project list, targets list, logs

### Distribution
- [ ] **P0** Code signing with Apple Developer certificate
- [ ] **P0** Notarization via `xcrun notarytool`
- [ ] **P0** DMG packaging with background image and drag-to-Applications
- [ ] **P1** Homebrew cask formula
- [ ] **P2** Sparkle or tauri-plugin-updater for auto-updates

---

## Phase 5: Growth (Month 7+)

- [ ] **P1** Pro tier + subscription infrastructure (Stripe)
- [ ] **P1** Team features: shared projects, shared targets, RBAC
- [ ] **P2** Web dashboard (companion, not replacement)
- [ ] **P2** Template library: common deployment patterns
- [ ] **P2** Community agent registry
- [ ] **P2** Windows/Linux builds (Tauri cross-platform)
- [ ] **P2** App Store distribution (sandboxing investigation)

---

## Technical Debt

- [ ] **P1** Test coverage: 23.27% overall — target 50%+ for runway-core
- [ ] **P1** Clippy: run `cargo clippy` with strict lints and fix all warnings
- [ ] **P2** Frontend tests: component tests for ProjectDetail, PasteAndDeploy, Targets
- [ ] **P2** CI/CD: GitHub Actions for `cargo test`, `cargo clippy`, `npm run build`
- [ ] **P2** Error handling: replace `.map_err(|e| e.to_string())` in commands.rs with proper error types

---

## Documentation

- [x] `docs/remote-agent.html` — Remote agent architecture and protocol docs
- [ ] **P1** `docs/mcp-server.html` — MCP server tools reference
- [ ] **P1** `docs/getting-started.html` — Quick start guide
- [ ] **P2** `docs/cli-reference.html` — CLI command reference
- [ ] **P2** README.md — Public-facing project README with screenshots
