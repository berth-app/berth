# Mac-Native Deployment Control Plane for AI-Generated Code

> "The Mac-native control plane for deploying and managing code that AI wrote for you"

## Problem Statement

AI code generation (Claude Code, Codex, Cursor, Windsurf) is exploding. Developers can now build crawlers, trading bots, AI agents, scrapers, and automation scripts in minutes. But then: **"Where do I run this?"**

- Vercel/Netlify are for websites, not long-running bots
- Vercel free tier prohibits commercial use; 800s max execution
- Railway is container-based, no native Mac UX
- Rundeck/PagerDuty is enterprise-only, web-based, $125/user/mo
- No native macOS app exists for this workflow

## Market Signals

- DevOps automation market: $14.4B (2025) -> $72.8B (2032), 26% CAGR
- Serverless market: $17.8B (2025) -> $124.5B (2034), 24% CAGR
- Railway: 2M developers, 10M deployments/month, $100M Series B (Jan 2026)
- ngrok: $26.7M ARR, bootstrapped, 88-person team
- Tailscale: $277M raised, $1.5B valuation
- Agentic traffic: up 6,900% YoY
- Bots now 52% of all web traffic
- 40% of enterprise apps will embed AI agents by end of 2026
- GitHub Agent HQ launched with Claude + Codex (Feb 2026)

## Architecture: Hub-and-Spoke

```
+--------------------------------------------------+
|         macOS Native App ("Mission Control")      |
|                                                    |
|  +----------+ +----------+ +-------------------+  |
|  | Deploy   | | Monitor  | | AI Code Import    |  |
|  | Wizard   | | Dashboard| | (paste from Claude |  |
|  |          | | Logs/CPU | |  Codex, Cursor)   |  |
|  +----------+ +----------+ +-------------------+  |
+--------+-----------+-------------+----------------+
         |           |             |
    +----+      +----+        +----+
    v           v             v
+--------+  +--------+  +--------------+
| Local  |  | Linux  |  |  Serverless  |
| Mac    |  | Agent  |  |  (User's     |
| Agent  |  | (VPS)  |  |   AWS/GCP)   |
+--------+  +--------+  +--------------+
                              |
              +---------------+---------------+
              v               v               v
         +---------+   +----------+   +-----------+
         | Lambda  |   | EC2 Spot |   | Cloudflare|
         | (cron   |   | (always  |   | Workers   |
         |  tasks) |   |  on bot) |   | (edge)    |
         +---------+   +----------+   +-----------+
```

### Key Architectural Insight: Zero Infrastructure Cost

You NEVER host their workloads. You deploy to:
- Their Mac (free, already running)
- Their Linux VPS (they pay $5-20/mo to Hetzner/DigitalOcean)
- Their AWS account (they pay AWS directly)
- Cloudflare Workers (they pay $5/mo to CF)

Your cost is purely software distribution. Like Terraform: you're the control plane, not the data plane.

## Target Personas

### 1. "The Vibe Coder" (Largest segment)
- Uses Claude Code or Cursor to build a scraper, bot, or automation
- Has working code on their Mac but no idea how to deploy it
- Needs: One-click deploy to somewhere that just works

### 2. "The Crypto/Trading Dev"
- Trading bots need 24/7 uptime, low latency
- Currently renting VPS ($20-50/mo) and SSH-ing in manually
- Needs: Deploy + monitor + restart from Mac app

### 3. "The AI Agent Builder"
- Building multi-step AI agents (research, scraping, data pipelines)
- Agents need persistent state, scheduled runs, webhook triggers
- Needs: Orchestration + execution environment

### 4. "The DevOps Pro" (Original Rundeck persona)
- Traditional runbook automation, server management
- Needs: Native Mac UI for operations work

## Killer Feature: "Paste & Deploy"

1. User builds a crawler in Claude Code
2. Opens the Mac app
3. Pastes code or points to local directory
4. App auto-detects: Python script, needs schedule, uses requests
5. Shows deploy options:
   - Run locally (free, instant)
   - Deploy to my Linux box (agent required)
   - Deploy as AWS Lambda (cron: every 6h)
   - Deploy to Cloudflare Worker (edge, always-on)
6. One click. Done. Live monitoring in the Mac app.

Nobody offers this today. Not Vercel, not Railway, not Render.

## Business Model: Transport-Gated Freemium (DRF-008)

### Pricing Tiers

| Feature | Free | Pro ($12/mo) | Team ($25/user/mo) |
|---------|------|-------------|-------------------|
| Local Mac execution | Unlimited | Unlimited | Unlimited |
| Direct-connect remote agents (gRPC) | Unlimited | Unlimited | Unlimited |
| NATS relay (NAT traversal, no ports) | — | 10 agents | Unlimited |
| Cloud targets (Lambda, Vercel, CF Workers) | — | Included | Included |
| Webhooks (inbound + outbound) | — | Included | Included |
| Shared projects | — | 3 projects | Unlimited |
| Template library (50+ projects) | Browse only | Deploy + use | Deploy + use |
| Curated marketplace (future) | Browse only | Buy + deploy | Buy + deploy |
| MCP server | Full access | Full access | Full access |
| Monitoring / execution history | Full | Full | Full |
| Managed compute (future) | — | — | Add-on |

**Free gate = transport type, not count.** Direct gRPC connectivity (LAN, VPN, static IP) is free with no limits. NATS relay, cloud targets, webhooks, sharing, and template deploy require Pro. MCP is fully free on all tiers — AI-agent adoption is the moat.

### Revenue Streams
1. **Pro Subscription** — NATS relay + cloud targets + webhooks + sharing + template deploy
2. **Team Subscription** — unlimited sharing, unlimited NATS agents, team collaboration
3. **Template Marketplace Fees** — curated community templates, 70/30 revenue share (Year 2+)
4. **Managed Compute Add-on** — spot instances on user's AWS/GCP (Year 3+)

## Revenue Projections

| Year    | Free Users | Paid  | Team Seats | ARR     |
|---------|-----------|-------|-----------|---------|
| Year 1  | 5,000     | 500   | -         | $72K    |
| Year 2  | 25,000    | 2,500 | 200       | $420K   |
| Year 3  | 100,000   | 8,000 | 1,000     | $1.5M   |

## Competitive Landscape

| Product                     | Type              | Long-running? | User's infra? | Mac native? | Price          |
|-----------------------------|-------------------|---------------|---------------|-------------|----------------|
| Vercel                      | Website hosting   | 800s max      | No            | No          | $20/mo+        |
| Railway                     | Container hosting | Yes           | No            | No          | Usage-based    |
| Rundeck/PagerDuty           | Enterprise ops    | Yes           | Yes           | No          | $125/user/mo   |
| Cloudflare Moltworker       | Edge agents       | Yes           | Sort of       | No          | $5/mo          |
| ngrok                       | Tunneling         | N/A           | Yes           | No          | Freemium       |
| **This Product**            | Deploy + Monitor  | Yes           | Yes           | **Yes**     | Free / $12/mo  |

## Comparable Exits / Benchmarks

- PagerDuty acquired Rundeck for $100M (2020)
- ngrok: $26.7M ARR, bootstrapped
- Tailscale: $1.5B valuation
- Railway: $100M Series B, 2M devs
- Tower (Git client): Profitable indie business at $69/yr

## Technical Stack (Recommended)

| Component         | Technology                                    |
|-------------------|-----------------------------------------------|
| Mac App           | SwiftUI + Swift Concurrency                   |
| Local Agent       | Go binary (cross-platform, single binary)     |
| Remote Agent      | Same Go binary, minimal footprint             |
| Lambda Runtime    | Go or Rust (fast cold starts)                 |
| Communication     | gRPC + mTLS (agent <-> app)                   |
| Discovery         | Bonjour (local) + Tailscale/WireGuard (remote)|
| Credential Store  | Apple Keychain (Mac) + encrypted vault (agents)|

## Key Risks

| Risk                          | Severity | Mitigation                                      |
|-------------------------------|----------|------------------------------------------------|
| Small TAM (Mac-only initially)| Medium   | Add Windows/Linux later; web dashboard Year 2   |
| Agent security concerns       | High     | Open-source the agent; mTLS; zero-trust         |
| Lambda cold starts hurt UX    | Medium   | Provisioned concurrency; show timing in UI       |
| Enterprise wants web-based    | High     | Web dashboard in Year 2 alongside Mac app        |
| Vercel/Railway add similar    | Medium   | Native Mac UX is hard to copy; agent protocol moat|

## Differentiation Moat

1. **Native macOS UX** — Tauri + Rust, Keychain, notifications, menu bar
2. **MCP-first** — AI coding agents can deploy via MCP (no other tool has this)
3. **Zero-infra model** — runs on user's compute, not yours (Terraform-like economics)
4. **Agent protocol** — lightweight Rust binary, single `curl | sh` install
5. **AI code import** — auto-detect runtime, dependencies, scheduling needs
6. **Timing** — AI code generation exploding, deployment is the bottleneck

## Distribution & Growth Strategy (DRF-009)

### Distribution Channels

| Channel | Timeline | Cost | Notes |
|---------|----------|------|-------|
| Direct download (DMG) | Launch | Free | Notarized + signed |
| Homebrew Cask | Launch | Free | `brew install --cask runway` |
| Product Hunt | Launch week | Free | Target: Top 5 of the day |
| Hacker News "Show HN" | Launch week | Free | Zero-infra angle |
| GitHub (agent repo) | Month 1 | Free | Source-available license (BSL) |
| MCP tool directories | Month 1 | Free | awesome-mcp, Claude Code listings |
| Setapp | Month 6-12 | ~30% rev share | Free-tier features only, Pro upsell |
| Mac App Store | Year 2+ | 15-30% commission | Deferred — sandbox challenges |

### Growth Playbook

**Pre-launch:** Build in public (X, Reddit, HN). Waitlist with 500-1000 emails.

**Launch week:** Product Hunt + Show HN + Reddit blitz (r/selfhosted, r/homelab, r/devops, r/ClaudeAI, r/algotrading).

**Sustained (Month 1-6):**
- AI coding tool integrations — tutorials, MCP directory listings, Anthropic/Cursor partnerships
- YouTube content — template showcases, "vibe coding to production" series
- Template library as SEO — each template = landing page ranking for long-tail searches
- GitHub agent repo — source-available, stars drive discovery

**Amplification (Month 6-12):** Setapp, dev newsletters (TLDR, Changelog), VPS provider partnerships (Hetzner, DigitalOcean), indie dev podcasts.

**Year 2+:** Mac App Store (lite version), open marketplace for community templates, web dashboard companion.

### Agent Licensing

Source-available (Business Source License or similar). Users can inspect what runs on their servers — critical for trust. License prevents commercial cloning. Mac app, MCP server, template library, and NATS relay remain fully proprietary.

### Setapp Strategy

Setapp includes Free-tier features only (local execution + direct-connect agents). NATS relay, cloud targets, webhooks, sharing, and template deploy still require a Runway Pro subscription. Distribution without cannibalizing Pro revenue.

---

*Research conducted March 2026. Sources include market data from Coherent Market Insights,
CB Insights, Railway docs, ngrok/Tailscale financials, GitHub Agent HQ announcements,
Cloudflare Moltworker launch, and PagerDuty/Rundeck acquisition filings.*
