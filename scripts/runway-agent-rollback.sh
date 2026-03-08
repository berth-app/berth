#!/bin/bash
# Runway Agent Auto-Rollback (ExecStopPost)
#
# Runs after the agent process exits. Handles three scenarios:
#
#   Exit 42 + .upgrading marker = intentional upgrade/rollback restart
#     → Clean up marker, let systemd restart with new binary
#
#   Exit 0 = clean shutdown
#     → Nothing to do
#
#   Any other exit during probation = crash after upgrade
#     → Copy backup binary over active, let systemd restart old version
#
# The '+' prefix on ExecStopPost runs this script as root.

# Resolve the runway user's home directory from the systemd service
RUNWAY_USER=$(systemctl show runway-agent -p User --value 2>/dev/null || echo "runway")
RUNWAY_HOME=$(eval echo "~${RUNWAY_USER}" 2>/dev/null || echo "/home/runway")

RUNWAY_DIR="${RUNWAY_HOME}/.runway"
BIN_DIR="${RUNWAY_DIR}/bin"
ACTIVE="${BIN_DIR}/runway-agent"
BACKUP="${BIN_DIR}/runway-agent.old"
PROBATION="${RUNWAY_DIR}/.probation"
UPGRADING="${RUNWAY_DIR}/.upgrading"
COUNT_FILE="${RUNWAY_DIR}/.rollback-count"

# systemd sets $EXIT_STATUS for ExecStopPost scripts.
# Fall back to $1 for manual invocation.
EXIT_CODE="${EXIT_STATUS:-${1:-0}}"

log_info()  { echo "runway-agent: $1" | systemd-cat -t runway-agent -p info  2>/dev/null || echo "$1"; }
log_warn()  { echo "runway-agent: $1" | systemd-cat -t runway-agent -p warning 2>/dev/null || echo "$1" >&2; }
log_err()   { echo "runway-agent: $1" | systemd-cat -t runway-agent -p err 2>/dev/null || echo "$1" >&2; }

# Exit 42 + .upgrading = intentional restart (upgrade or rollback)
if [ "$EXIT_CODE" = "42" ] && [ -f "$UPGRADING" ]; then
    log_info "Intentional restart (exit 42), cleaning up markers"
    rm -f "$UPGRADING"
    exit 0
fi

# Clean exit = nothing to do
[ "$EXIT_CODE" = "0" ] && exit 0

# Not in probation = normal crash, systemd just restarts
if [ ! -f "$PROBATION" ]; then
    exit 0
fi

# ---- Crash during probation: auto-rollback ----

COUNT=$(cat "$COUNT_FILE" 2>/dev/null || echo 0)

# Loop prevention: give up after 2 rollback attempts
if [ "$COUNT" -ge 2 ]; then
    log_err "Rollback loop detected (count=$COUNT), giving up"
    rm -f "$PROBATION" "$COUNT_FILE" "$UPGRADING"
    exit 1
fi

if [ -f "$BACKUP" ]; then
    # Use cp (not mv) so the backup survives for manual recovery
    if cp "$BACKUP" "$ACTIVE" && chmod +x "$ACTIVE"; then
        chown "${RUNWAY_USER}:${RUNWAY_USER}" "$ACTIVE"
        echo $((COUNT + 1)) > "$COUNT_FILE"
        rm -f "$PROBATION" "$UPGRADING"
        log_warn "Auto-rollback from crash (exit=$EXIT_CODE, attempt=$((COUNT + 1)))"
    else
        log_err "Failed to copy backup binary"
        rm -f "$PROBATION"
        exit 1
    fi
else
    log_err "Probation crash but no backup at $BACKUP"
    rm -f "$PROBATION"
    exit 1
fi
