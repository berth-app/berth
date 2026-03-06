#!/bin/bash
set -euo pipefail

# Runway Agent Installer
# Usage:
#   Install:    curl -sSL https://get.runway.dev | bash
#   Uninstall:  curl -sSL https://get.runway.dev | bash -s -- --uninstall

BINARY_NAME="runway-agent"
INSTALL_DIR="/usr/local/bin"
INSTALL_PATH="${INSTALL_DIR}/${BINARY_NAME}"
SERVICE_NAME="runway-agent"
SERVICE_FILE="/etc/systemd/system/${SERVICE_NAME}.service"
AGENT_USER="runway"
AGENT_PORT="50051"
BASE_URL="https://get.runway.dev/releases/latest"

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
  info "Uninstalling Runway agent..."

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

  # Remove the binary
  if [ -f "${INSTALL_PATH}" ]; then
    info "Removing binary ${INSTALL_PATH}..."
    rm -f "${INSTALL_PATH}"
  fi

  # Optionally remove the runway user
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

  ok "Runway agent uninstalled."
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
# Download the agent binary
# ---------------------------------------------------------------------------

download_binary() {
  local url="${BASE_URL}/${BINARY_NAME}-${ARCH}-unknown-linux-gnu"
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
  ok "Binary installed to ${INSTALL_PATH}"
}

# ---------------------------------------------------------------------------
# Create the runway system user
# ---------------------------------------------------------------------------

create_user() {
  if id "${AGENT_USER}" &>/dev/null; then
    info "User '${AGENT_USER}' already exists, skipping creation."
  else
    info "Creating system user '${AGENT_USER}'..."
    useradd --system --no-create-home --shell /usr/sbin/nologin "${AGENT_USER}"
    ok "User '${AGENT_USER}' created."
  fi
}

# ---------------------------------------------------------------------------
# Create and enable the systemd service
# ---------------------------------------------------------------------------

install_service() {
  info "Creating systemd service at ${SERVICE_FILE}..."

  cat > "${SERVICE_FILE}" <<EOF
[Unit]
Description=Runway Agent
After=network.target

[Service]
Type=simple
User=${AGENT_USER}
ExecStart=${INSTALL_PATH} --port ${AGENT_PORT}
Restart=always
RestartSec=5
Environment=RUST_LOG=info

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
  info "Installing Runway agent..."
  need_root
  detect_os
  detect_arch
  download_binary
  create_user
  install_service

  echo ""
  ok "Runway agent is running on port ${AGENT_PORT}."
  info "Add this target in Runway with your server's IP:"
  echo ""
  echo "    runway targets add my-server --host <SERVER_IP> --port ${AGENT_PORT}"
  echo ""
  info "Useful commands:"
  echo "    systemctl status ${SERVICE_NAME}    # check status"
  echo "    journalctl -u ${SERVICE_NAME} -f    # follow logs"
  echo "    sudo bash install-agent.sh --uninstall  # remove"
  echo ""
}

main
