#!/bin/bash
set -euo pipefail

# Berth NATS Relay Setup Script
# Prerequisites: Synadia Cloud account (https://cloud.synadia.com)
#
# Usage:
#   ./scripts/setup-nats.sh install-tools       # Install nsc + nats CLI
#   ./scripts/setup-nats.sh init                 # Initialize operator/account (Synadia Cloud)
#   ./scripts/setup-nats.sh add-agent <name>     # Create agent user + credentials
#   ./scripts/setup-nats.sh add-desktop          # Create desktop user + credentials
#   ./scripts/setup-nats.sh create-streams <url> # Create JetStream streams
#   ./scripts/setup-nats.sh test <url> <creds>   # Test NATS connectivity
#   ./scripts/setup-nats.sh status               # Show current setup status

BERTH_DIR="$HOME/.berth"
NATS_DIR="$BERTH_DIR/nats"

info()  { echo "==> $*"; }
error() { echo "ERROR: $*" >&2; exit 1; }

cmd_install_tools() {
    info "Installing NATS CLI tools via Homebrew..."

    if ! command -v brew &>/dev/null; then
        error "Homebrew is required. Install from https://brew.sh"
    fi

    brew tap nats-io/nats-tools 2>/dev/null || true
    brew install nats-io/nats-tools/nsc nats-io/nats-tools/nats

    info "Installed:"
    nsc --version 2>/dev/null || echo "  nsc: failed"
    nats --version 2>/dev/null || echo "  nats: failed"
}

cmd_init() {
    info "Initializing NATS configuration for Berth..."
    info ""
    info "Before running this, you need to:"
    info "  1. Sign up at https://cloud.synadia.com"
    info "  2. Create a Team and System"
    info "  3. Download your account credentials (.creds file) from the dashboard"
    info ""
    info "Synadia Cloud provides the operator and account automatically."
    info "You only need to create users (agents + desktop) using nsc."
    info ""

    mkdir -p "$NATS_DIR"

    # Check if nsc operator exists
    if nsc list operators 2>/dev/null | grep -q "berth"; then
        info "Operator 'berth' already exists"
    else
        info "To connect nsc to your Synadia Cloud account, run:"
        info "  nsc login"
        info ""
        info "This will open a browser to authenticate with Synadia Cloud"
        info "and sync your operator/account configuration."
    fi
}

cmd_add_agent() {
    local name="${1:?Usage: setup-nats.sh add-agent <agent-name>}"

    info "Creating NATS user for agent: $name"

    mkdir -p "$NATS_DIR"

    # Create user with publish permissions scoped to its own subjects
    nsc add user "$name" \
        --allow-pub "berth.${name}.>" \
        --deny-sub ">" \
        2>/dev/null || info "User $name may already exist, regenerating creds..."

    # Generate credentials file
    local creds_file="$NATS_DIR/${name}.creds"
    nsc generate creds -n "$name" > "$creds_file"
    chmod 600 "$creds_file"

    info "Credentials saved to: $creds_file"
    info ""
    info "To deploy to the agent server:"
    info "  scp $creds_file user@server:~/.berth/nats.creds"
    info ""
    info "Then start the agent with:"
    info "  berth-agent --listen-all --nats-url tls://connect.ngs.global --nats-creds ~/.berth/nats.creds --nats-agent-id $name"
}

cmd_add_desktop() {
    info "Creating NATS user for desktop app..."

    mkdir -p "$NATS_DIR"

    # Desktop can subscribe to all berth subjects but cannot publish
    nsc add user desktop \
        --allow-sub "berth.>" \
        --deny-pub ">" \
        2>/dev/null || info "User 'desktop' may already exist, regenerating creds..."

    local creds_file="$NATS_DIR/desktop.creds"
    nsc generate creds -n desktop > "$creds_file"
    chmod 600 "$creds_file"

    info "Credentials saved to: $creds_file"
    info ""
    info "Configure in Berth app Settings:"
    info "  NATS URL:   tls://connect.ngs.global"
    info "  NATS Creds: $creds_file"
}

cmd_create_streams() {
    local url="${1:?Usage: setup-nats.sh create-streams <nats-url>}"
    local creds="${2:-$NATS_DIR/desktop.creds}"

    info "Creating JetStream streams..."

    if [ ! -f "$creds" ]; then
        error "Credentials file not found: $creds. Run 'add-desktop' first."
    fi

    # Events stream: durable, 7-day retention
    nats --server "$url" --creds "$creds" stream add BERTH_EVENTS \
        --subjects "berth.*.event.>" \
        --retention work \
        --max-age "7d" \
        --max-msgs 100000 \
        --max-bytes 268435456 \
        --storage file \
        --replicas 1 \
        --discard old \
        --no-deny-delete \
        --no-deny-purge \
        --defaults \
        2>/dev/null && info "Stream BERTH_EVENTS created" \
        || info "Stream BERTH_EVENTS may already exist"

    # Logs stream: shorter retention, higher volume
    nats --server "$url" --creds "$creds" stream add BERTH_LOGS \
        --subjects "berth.*.log.>" \
        --retention limits \
        --max-age "24h" \
        --max-msgs-per-subject 50000 \
        --max-bytes 536870912 \
        --storage file \
        --replicas 1 \
        --discard old \
        --no-deny-delete \
        --no-deny-purge \
        --defaults \
        2>/dev/null && info "Stream BERTH_LOGS created" \
        || info "Stream BERTH_LOGS may already exist"

    info ""
    info "Streams created. Verify with:"
    info "  nats --server $url --creds $creds stream ls"
}

cmd_test() {
    local url="${1:?Usage: setup-nats.sh test <nats-url> <creds-file>}"
    local creds="${2:?Usage: setup-nats.sh test <nats-url> <creds-file>}"

    if [ ! -f "$creds" ]; then
        error "Credentials file not found: $creds"
    fi

    info "Testing NATS connection to $url..."

    # Test basic connectivity
    if nats --server "$url" --creds "$creds" server ping --count 1 2>/dev/null; then
        info "Connection OK"
    else
        error "Connection failed. Check URL and credentials."
    fi

    # Test JetStream
    info "Checking JetStream streams..."
    nats --server "$url" --creds "$creds" stream ls 2>/dev/null || info "No streams found (run create-streams)"

    # Test publish/subscribe
    info "Testing publish..."
    echo '{"test":true}' | nats --server "$url" --creds "$creds" pub berth.test.heartbeat 2>/dev/null \
        && info "Publish OK" \
        || info "Publish test failed (may be expected if user lacks publish permissions)"
}

cmd_status() {
    info "Berth NATS Setup Status"
    info "========================"

    echo ""
    echo "CLI Tools:"
    echo "  nsc:  $(command -v nsc 2>/dev/null && nsc --version 2>/dev/null || echo 'not installed')"
    echo "  nats: $(command -v nats 2>/dev/null && nats --version 2>/dev/null || echo 'not installed')"

    echo ""
    echo "Credentials ($NATS_DIR):"
    if [ -d "$NATS_DIR" ]; then
        ls -la "$NATS_DIR"/*.creds 2>/dev/null || echo "  No .creds files found"
    else
        echo "  Directory does not exist"
    fi

    echo ""
    echo "nsc Operators:"
    nsc list operators 2>/dev/null || echo "  No operators configured"
}

# Main dispatch
case "${1:-help}" in
    install-tools)  cmd_install_tools ;;
    init)           cmd_init ;;
    add-agent)      cmd_add_agent "${2:-}" ;;
    add-desktop)    cmd_add_desktop ;;
    create-streams) cmd_create_streams "${2:-}" "${3:-}" ;;
    test)           cmd_test "${2:-}" "${3:-}" ;;
    status)         cmd_status ;;
    help|*)
        echo "Berth NATS Relay Setup"
        echo ""
        echo "Usage: $0 <command> [args]"
        echo ""
        echo "Commands:"
        echo "  install-tools            Install nsc + nats CLI via Homebrew"
        echo "  init                     Initialize NATS configuration"
        echo "  add-agent <name>         Create agent user + credentials"
        echo "  add-desktop              Create desktop user + credentials"
        echo "  create-streams <url>     Create JetStream streams"
        echo "  test <url> <creds>       Test NATS connectivity"
        echo "  status                   Show current setup status"
        echo ""
        echo "Quick start:"
        echo "  1. Sign up at https://cloud.synadia.com"
        echo "  2. $0 install-tools"
        echo "  3. $0 init              (then: nsc login)"
        echo "  4. $0 add-agent my-vps"
        echo "  5. $0 add-desktop"
        echo "  6. $0 create-streams tls://connect.ngs.global"
        ;;
esac
