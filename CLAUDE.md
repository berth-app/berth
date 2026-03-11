# Berth - Project Instructions

## Git Workflow

### Branch Strategy
- **`main`** — Release branch. What ships via Homebrew (`brew install --cask berth`). Protected: PRs only, enforced for admins, no force push.
- **`dev`** — Integration branch. All feature work merges here first. Protected: PRs only, enforced for admins, no force push.
- **Feature branches** — `feat/short-description`, `fix/short-description`. Branch from `dev`, PR back to `dev`.

### Flow
```
feat/my-feature  →  PR to dev  →  validate & accumulate  →  PR to main (with version bump)
```

1. Create feature branch from `dev`
2. Develop, commit, push
3. Open PR targeting `dev`
4. Once several changes are validated on `dev`, open PR from `dev` → `main`
5. Bump version in the `dev` → `main` PR
6. Tag release on `main` (`app-v*` triggers CI build + Homebrew update)

### Rules
- **Never push directly to `main` or `dev`** — branch protection enforced for admins
- **Never PR feature branches directly to `main`** — always go through `dev`
- One logical change per commit
- Commit messages: imperative mood, max 72 chars first line

## Architecture

```
crates/berth-agent/    # Rust binary for remote Linux targets (systemd service)
crates/berth-core/     # Shared Rust library (business logic, gRPC, models)
crates/berth-proto/    # Shared contract crate (proto types, NATS relay, message auth)
crates/berth-cli/      # CLI interface (thin wrapper over core)
crates/berth-mcp/      # MCP server implementation (Rust)
src-tauri/             # Tauri 2.0 app backend (Rust commands)
src/                   # React 19 + TypeScript frontend
proto/                 # Protobuf definitions
```

## Tech Stack
- **Desktop:** Tauri 2.0 + React 19 + TypeScript + shadcn/ui + Tailwind CSS
- **Backend/Agent:** Rust (tokio + tonic + axum)
- **Communication:** gRPC + NATS (Synadia Cloud)
- **DB:** SQLite (rusqlite)
- **Terminal:** xterm.js

## Coding Standards
- React 19 functional components, strict TypeScript, Tailwind CSS
- Rust: async tokio, clippy strict, no unsafe, thiserror/anyhow, tracing
- No comments explaining obvious code — comment WHY, not WHAT
