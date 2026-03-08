#!/bin/bash
# Runway Agent Auto-Rollback (ExecStopPost)
#
# Runs after the agent process exits. If the agent was in probation
# (just upgraded) and exited abnormally, swaps the backup binary back
# so systemd restarts the old known-good version.
#
# Exit code meanings:
#   0  = clean shutdown (probation passed or normal stop)
#   42 = agent self-rolled-back (Layer 2), just restart
#   *  = crash during probation → swap backup (Layer 1)

RUNWAY_DIR="/home/runway/.runway"
PROBATION="$RUNWAY_DIR/.probation"
BACKUP="$RUNWAY_DIR/runway-agent.old"
BINARY="/usr/local/bin/runway-agent"
COUNT_FILE="$RUNWAY_DIR/.rollback-count"

EXIT_CODE="${1:-0}"

# Not in probation — nothing to do
[ ! -f "$PROBATION" ] && exit 0

# Clean exit — probation will be cleared by the agent itself
[ "$EXIT_CODE" = "0" ] && exit 0

# Exit 42 = agent already handled its own rollback, just clean up
if [ "$EXIT_CODE" = "42" ]; then
    rm -f "$PROBATION"
    exit 0
fi

# Crash during probation — perform Layer 1 rollback
COUNT=$(cat "$COUNT_FILE" 2>/dev/null || echo 0)

# Loop prevention: stop after 2 rollback attempts
if [ "$COUNT" -ge 2 ]; then
    rm -f "$PROBATION" "$COUNT_FILE"
    echo "runway-agent: rollback loop detected (count=$COUNT), giving up" | systemd-cat -t runway-agent -p err
    exit 1
fi

if [ -f "$BACKUP" ]; then
    cp "$BACKUP" "$BINARY" && chmod +x "$BINARY"
    echo $((COUNT + 1)) > "$COUNT_FILE"
    rm -f "$PROBATION"
    echo "runway-agent: auto-rollback from crash (exit=$EXIT_CODE, attempt=$((COUNT + 1)))" | systemd-cat -t runway-agent -p warning
else
    echo "runway-agent: probation crash but no backup binary found" | systemd-cat -t runway-agent -p err
    rm -f "$PROBATION"
    exit 1
fi
