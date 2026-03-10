#!/bin/bash
set -euo pipefail

# Berth Agent Installer
# Usage:
#   Install:    curl -sSL https://agent.getberth.dev/install.sh | sudo bash
#   Uninstall:  curl -sSL https://agent.getberth.dev/install.sh | sudo bash -s -- --uninstall

BINARY_NAME="berth-agent"
SERVICE_NAME="berth-agent"
SERVICE_FILE="/etc/systemd/system/${SERVICE_NAME}.service"
AGENT_USER="berth"
AGENT_HOME=""      # resolved after user creation
BERTH_DIR=""      # resolved after user creation
INSTALL_PATH=""    # resolved after user creation
BASE_URL="https://github.com/berth-app/berth-agent/releases/latest/download"
ROLLBACK_SCRIPT_DIR="/usr/local/lib/berth"
ROLLBACK_SCRIPT_PATH="${ROLLBACK_SCRIPT_DIR}/rollback.sh"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

info()  { printf "\033[1;34m[info]\033[0m  %s\n" "$1"; }
ok()    { printf "\033[1;32m[ok]\033[0m    %s\n" "$1"; }
err()   { printf "\033[1;31m[error]\033[0m %s\n" "$1" >&2; }

need_root() {
  if [ "$(id -u)" -ne 0 ]; then
    err "This script must be run as root. Try: sudo bash install-agent.sh"
    exit 1
  fi
}

# ---------------------------------------------------------------------------
# Uninstall
# ---------------------------------------------------------------------------

uninstall() {
  need_root
  info "Uninstalling Berth agent..."

  # Stop and disable the systemd service
  if systemctl is-active --quiet "${SERVICE_NAME}" 2>/dev/null; then
    info "Stopping ${SERVICE_NAME} service..."
    systemctl stop "${SERVICE_NAME}"
  fi
  if systemctl is-enabled --quiet "${SERVICE_NAME}" 2>/dev/null; then
    info "Disabling ${SERVICE_NAME} service..."
    systemctl disable "${SERVICE_NAME}"
  fi

  # Remove the service file
  if [ -f "${SERVICE_FILE}" ]; then
    info "Removing service file ${SERVICE_FILE}..."
    rm -f "${SERVICE_FILE}"
    systemctl daemon-reload
  fi

  # Remove the binary (check both old and new locations)
  local agent_home
  agent_home=$(eval echo "~${AGENT_USER}" 2>/dev/null || echo "")
  for bin_path in "/usr/local/bin/${BINARY_NAME}" "${agent_home}/.berth/bin/${BINARY_NAME}"; do
    if [ -f "${bin_path}" ]; then
      info "Removing binary ${bin_path}..."
      rm -f "${bin_path}"
    fi
  done

  # Remove the rollback script
  if [ -f "${ROLLBACK_SCRIPT_PATH}" ]; then
    info "Removing rollback script ${ROLLBACK_SCRIPT_PATH}..."
    rm -f "${ROLLBACK_SCRIPT_PATH}"
    rmdir "${ROLLBACK_SCRIPT_DIR}" 2>/dev/null || true
  fi

  # Optionally remove the berth user
  if id "${AGENT_USER}" &>/dev/null; then
    printf "Remove the '%s' system user? [y/N] " "${AGENT_USER}"
    read -r answer
    if [ "${answer}" = "y" ] || [ "${answer}" = "Y" ]; then
      userdel "${AGENT_USER}" 2>/dev/null || true
      ok "User '${AGENT_USER}' removed."
    else
      info "Keeping user '${AGENT_USER}'."
    fi
  fi

  ok "Berth agent uninstalled."
  exit 0
}

# Handle --uninstall flag before anything else
if [ "${1:-}" = "--uninstall" ]; then
  uninstall
fi

# ---------------------------------------------------------------------------
# OS Detection — Linux only
# ---------------------------------------------------------------------------

detect_os() {
  local os
  os="$(uname -s)"
  case "${os}" in
    Linux)  info "Detected OS: Linux" ;;
    Darwin) err "macOS is not supported. The agent is for Linux servers only."; exit 1 ;;
    *)      err "Unsupported OS: ${os}. The agent runs on Linux only."; exit 1 ;;
  esac
}

# ---------------------------------------------------------------------------
# Architecture Detection
# ---------------------------------------------------------------------------

detect_arch() {
  local machine
  machine="$(uname -m)"
  case "${machine}" in
    x86_64)  ARCH="x86_64" ;;
    aarch64) ARCH="aarch64" ;;
    arm64)   ARCH="aarch64" ;;
    *)       err "Unsupported architecture: ${machine}. Supported: x86_64, aarch64."; exit 1 ;;
  esac
  info "Detected architecture: ${ARCH}"
}

# ---------------------------------------------------------------------------
# Create the berth system user (idempotent)
# ---------------------------------------------------------------------------

create_user() {
  if id "${AGENT_USER}" &>/dev/null; then
    info "User '${AGENT_USER}' already exists, skipping creation."
  else
    info "Creating system user '${AGENT_USER}'..."
    useradd --system --create-home --shell /usr/sbin/nologin "${AGENT_USER}"
    ok "User '${AGENT_USER}' created."
  fi

  # Resolve home dir and set paths
  AGENT_HOME=$(eval echo "~${AGENT_USER}")
  BERTH_DIR="${AGENT_HOME}/.berth"
  INSTALL_PATH="${BERTH_DIR}/bin/${BINARY_NAME}"

  # Create directory structure
  mkdir -p "${BERTH_DIR}/bin" "${BERTH_DIR}/deploys"
  chown -R "${AGENT_USER}:${AGENT_USER}" "${BERTH_DIR}"
}

# ---------------------------------------------------------------------------
# Download the agent binary
# ---------------------------------------------------------------------------

download_binary() {
  local url="${BASE_URL}/${BINARY_NAME}-linux-${ARCH}"
  info "Downloading ${BINARY_NAME} from ${url}..."

  if command -v curl &>/dev/null; then
    curl -fsSL -o "${INSTALL_PATH}" "${url}"
  elif command -v wget &>/dev/null; then
    wget -qO "${INSTALL_PATH}" "${url}"
  else
    err "Neither curl nor wget found. Install one and try again."
    exit 1
  fi

  chmod +x "${INSTALL_PATH}"
  chown "${AGENT_USER}:${AGENT_USER}" "${INSTALL_PATH}"
  ok "Binary installed to ${INSTALL_PATH}"
}

# ---------------------------------------------------------------------------
# Install the auto-rollback script
# ---------------------------------------------------------------------------

install_rollback_script() {
  info "Installing rollback script to ${ROLLBACK_SCRIPT_PATH}..."
  install -d "${ROLLBACK_SCRIPT_DIR}"

  # Prefer bundled script next to the installer, otherwise download
  local local_script
  local_script="$(dirname "$0")/berth-agent-rollback.sh"
  if [ -f "${local_script}" ]; then
    install -m 755 "${local_script}" "${ROLLBACK_SCRIPT_PATH}"
  else
    local url="https://raw.githubusercontent.com/berth-app/berth-agent/main/rollback.sh"
    if command -v curl &>/dev/null; then
      curl -fsSL -o "${ROLLBACK_SCRIPT_PATH}" "${url}"
    elif command -v wget &>/dev/null; then
      wget -qO "${ROLLBACK_SCRIPT_PATH}" "${url}"
    else
      err "Cannot install rollback script (no curl/wget)."
      exit 1
    fi
    chmod 755 "${ROLLBACK_SCRIPT_PATH}"
  fi

  ok "Rollback script installed to ${ROLLBACK_SCRIPT_PATH}"
}

# ---------------------------------------------------------------------------
# Generate environment file (idempotent — won't overwrite existing)
# ---------------------------------------------------------------------------

install_env_file() {
  local env_file="${BERTH_DIR}/agent.env"
  if [ -f "${env_file}" ]; then
    info "Environment file already exists, skipping."
    return
  fi

  info "Creating environment file at ${env_file}..."
  cat > "${env_file}" <<'ENVEOF'
# Berth Agent Configuration
# Uncomment and edit the lines you need.

# NATS relay (enables zero-port remote control from desktop app)
# BERTH_NATS_URL=tls://connect.ngs.global
# BERTH_NATS_CREDS=/home/berth/.berth/nats.creds
# BERTH_NATS_AGENT_ID=my-server

# gRPC port (default 50051)
# BERTH_PORT=50051

# Log level
RUST_LOG=info
ENVEOF

  chown "${AGENT_USER}:${AGENT_USER}" "${env_file}"
  ok "Environment file created at ${env_file}"
}

# ---------------------------------------------------------------------------
# Create and enable the systemd service (idempotent)
# ---------------------------------------------------------------------------

install_service() {
  info "Creating systemd service at ${SERVICE_FILE}..."

  cat > "${SERVICE_FILE}" <<EOF
[Unit]
Description=Berth Deployment Agent
After=network-online.target
Wants=network-online.target
StartLimitBurst=5
StartLimitIntervalSec=120

[Service]
Type=simple
User=${AGENT_USER}
Group=${AGENT_USER}

ExecStart=${INSTALL_PATH}
ExecStopPost=+${ROLLBACK_SCRIPT_PATH}

EnvironmentFile=-${BERTH_DIR}/agent.env

Restart=always
RestartSec=5

# Exit code 42 = intentional upgrade/rollback restart.
# Treated as success so it doesn't count toward StartLimitBurst.
SuccessExitStatus=42

# Security hardening
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=${BERTH_DIR}

[Install]
WantedBy=multi-user.target
EOF

  systemctl daemon-reload
  systemctl enable "${SERVICE_NAME}"
  systemctl start "${SERVICE_NAME}"
  ok "Service '${SERVICE_NAME}' enabled and started."
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

main() {
  info "Installing Berth agent..."
  need_root
  detect_os
  detect_arch
  create_user
  download_binary
  install_rollback_script
  install_env_file
  install_service

  echo ""
  ok "Berth agent is running."

  # Wait briefly and extract pairing code from journal
  sleep 3
  local pairing_code
  pairing_code=$(journalctl -u "${SERVICE_NAME}" --no-pager -n 50 2>/dev/null | grep -oP '(?<=\[PAIRING\] Code: )[A-Z0-9]{6}' | tail -1 || true)

  if [ -n "${pairing_code}" ]; then
    echo ""
    printf "\033[1;33m┌────────────────────────────────────────┐\033[0m\n"
    printf "\033[1;33m│  Pairing code:  \033[1;37m%-6s\033[1;33m                  │\033[0m\n" "${pairing_code}"
    printf "\033[1;33m│  Valid for 15 minutes                  │\033[0m\n"
    printf "\033[1;33m│                                        │\033[0m\n"
    printf "\033[1;33m│  Enter in Berth → Targets → Pair Agent │\033[0m\n"
    printf "\033[1;33m└────────────────────────────────────────┘\033[0m\n"
    echo ""
  else
    info "Configure NATS relay for remote control:"
    echo "    sudo nano ${BERTH_DIR}/agent.env"
    echo ""
    info "Once NATS is configured, restart the agent to get a pairing code:"
    echo "    sudo systemctl restart ${SERVICE_NAME}"
    echo "    journalctl -u ${SERVICE_NAME} -f   # look for [PAIRING] Code: XXXXXX"
    echo ""
  fi

  info "Useful commands:"
  echo "    systemctl status ${SERVICE_NAME}    # check status"
  echo "    journalctl -u ${SERVICE_NAME} -f    # follow logs"
  echo "    ${INSTALL_PATH} update              # self-update"
  echo "    sudo bash install-agent.sh --uninstall  # remove"
  echo ""
}

main
