#!/usr/bin/env bash
set -euo pipefail

APP_NAME="cc-switch-web"
DEFAULT_BIND="[::]:3650,0.0.0.0:3650"
PNPM_VERSION="9.15.9"
NODE_MAJOR="20"
RUST_TOOLCHAIN="1.95"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BIN="$APP_DIR/src-tauri/target/release/cc-switch"
DIST="$APP_DIR/dist"
START_SCRIPT="$APP_DIR/scripts/cc-switch-web.sh"

CC_SWITCH_WEB_BIND="${CC_SWITCH_WEB_BIND:-$DEFAULT_BIND}"
CC_SWITCH_WEB_DIST="${CC_SWITCH_WEB_DIST:-$DIST}"

log() {
  printf '\033[1;32m[INFO]\033[0m %s\n' "$*"
}

warn() {
  printf '\033[1;33m[WARN]\033[0m %s\n' "$*"
}

die() {
  printf '\033[1;31m[ERROR]\033[0m %s\n' "$*" >&2
  exit 1
}

has_cmd() {
  command -v "$1" >/dev/null 2>&1
}

as_root() {
  if [[ "$(id -u)" -eq 0 ]]; then
    "$@"
  elif has_cmd sudo; then
    sudo "$@"
  else
    die "Need root permission. Please run as root or install sudo."
  fi
}

detect_os() {
  if [[ ! -f /etc/os-release ]]; then
    die "Cannot detect Linux distribution: /etc/os-release not found."
  fi

  # shellcheck disable=SC1091
  . /etc/os-release
  OS_ID="${ID:-unknown}"
  OS_LIKE="${ID_LIKE:-}"
  OS_VERSION="${VERSION_ID:-unknown}"

  log "Detected OS: ${PRETTY_NAME:-$OS_ID $OS_VERSION}"
}

install_system_deps_debian() {
  log "Installing system dependencies with apt..."
  as_root apt-get update
  as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y \
    build-essential curl wget file unzip ca-certificates pkg-config libssl-dev \
    libwebkit2gtk-4.1-dev libxdo-dev libayatana-appindicator3-dev librsvg2-dev \
    git
}

install_system_deps_fedora() {
  local pkg="dnf"
  has_cmd dnf || pkg="yum"
  log "Installing system dependencies with $pkg..."
  as_root "$pkg" install -y \
    gcc gcc-c++ make curl wget file unzip ca-certificates pkgconfig openssl-devel \
    webkit2gtk4.1-devel libxdo-devel libappindicator-gtk3-devel librsvg2-devel \
    git
}

install_system_deps_arch() {
  log "Installing system dependencies with pacman..."
  as_root pacman -Sy --needed --noconfirm \
    base-devel curl wget file unzip ca-certificates pkgconf openssl \
    webkit2gtk-4.1 xdotool libappindicator-gtk3 librsvg git
}

install_system_deps() {
  detect_os

  case "$OS_ID" in
    ubuntu|debian|linuxmint|pop|elementary)
      install_system_deps_debian
      ;;
    fedora|rhel|centos|rocky|almalinux|ol)
      install_system_deps_fedora
      ;;
    arch|manjaro|endeavouros)
      install_system_deps_arch
      ;;
    *)
      case "$OS_LIKE" in
        *debian*) install_system_deps_debian ;;
        *fedora*|*rhel*) install_system_deps_fedora ;;
        *arch*) install_system_deps_arch ;;
        *)
          die "Unsupported Linux distribution: $OS_ID. Please install Tauri Linux dependencies manually."
          ;;
      esac
      ;;
  esac
}

install_rust() {
  if has_cmd cargo && has_cmd rustup; then
    log "Rust is already installed: $(rustc --version)"
  else
    log "Installing Rust with rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  fi

  # shellcheck disable=SC1091
  [[ -f "$HOME/.cargo/env" ]] && . "$HOME/.cargo/env"

  has_cmd rustup || die "rustup was not found after installation."
  has_cmd cargo || die "cargo was not found after installation."

  log "Installing Rust toolchain $RUST_TOOLCHAIN..."
  rustup toolchain install "$RUST_TOOLCHAIN"
  rustup default "$RUST_TOOLCHAIN"
}

install_node_debian() {
  log "Installing Node.js $NODE_MAJOR from NodeSource..."
  curl -fsSL "https://deb.nodesource.com/setup_${NODE_MAJOR}.x" | as_root bash -
  as_root apt-get install -y nodejs
}

install_node_fedora() {
  local pkg="dnf"
  has_cmd dnf || pkg="yum"
  log "Installing Node.js with $pkg..."
  as_root "$pkg" install -y nodejs npm
}

install_node_arch() {
  log "Installing Node.js with pacman..."
  as_root pacman -Sy --needed --noconfirm nodejs npm
}

install_node() {
  if has_cmd node; then
    log "Node.js is already installed: $(node -v)"
  else
    case "$OS_ID" in
      ubuntu|debian|linuxmint|pop|elementary) install_node_debian ;;
      fedora|rhel|centos|rocky|almalinux|ol) install_node_fedora ;;
      arch|manjaro|endeavouros) install_node_arch ;;
      *)
        case "$OS_LIKE" in
          *debian*) install_node_debian ;;
          *fedora*|*rhel*) install_node_fedora ;;
          *arch*) install_node_arch ;;
          *) die "Cannot install Node.js automatically on this OS." ;;
        esac
        ;;
    esac
  fi

  has_cmd node || die "node was not found after installation."
}

install_pnpm() {
  log "Preparing pnpm $PNPM_VERSION..."

  if has_cmd corepack; then
    corepack enable || true
    corepack prepare "pnpm@$PNPM_VERSION" --activate
  fi

  if ! has_cmd pnpm; then
    as_root npm install -g "pnpm@$PNPM_VERSION"
  fi

  log "pnpm version: $(pnpm -v)"
}

install_frontend_deps() {
  log "Installing frontend dependencies..."
  cd "$APP_DIR"

  # pnpm v10+ may block build scripts by default. Use pnpm 9 for a fully
  # non-interactive server deployment.
  pnpm install --frozen-lockfile
}

build_frontend() {
  log "Building frontend renderer..."
  cd "$APP_DIR"
  pnpm build:renderer

  [[ -f "$DIST/index.html" ]] || die "Frontend build failed: $DIST/index.html not found."
}

build_backend() {
  log "Building Rust backend..."
  cd "$APP_DIR/src-tauri"

  # shellcheck disable=SC1091
  [[ -f "$HOME/.cargo/env" ]] && . "$HOME/.cargo/env"

  cargo build --release
  [[ -x "$BIN" ]] || die "Backend build failed: $BIN not found."
}

ensure_start_script() {
  [[ -f "$START_SCRIPT" ]] || die "Start script not found: $START_SCRIPT"
  chmod +x "$START_SCRIPT"
}

start_service() {
  log "Starting $APP_NAME..."
  cd "$APP_DIR"
  ensure_start_script
  CC_SWITCH_WEB_BIND="$CC_SWITCH_WEB_BIND" \
  CC_SWITCH_WEB_DIST="$CC_SWITCH_WEB_DIST" \
  "$START_SCRIPT" restart
}

show_result() {
  cat <<EOF

Deployment finished.

Project: $APP_DIR
Bind:    $CC_SWITCH_WEB_BIND
IPv4:    http://YOUR_SERVER_IPV4:3650
IPv6:    http://[YOUR_SERVER_IPV6]:3650

Useful commands:
  cd $APP_DIR
  ./scripts/cc-switch-web.sh status
  ./scripts/cc-switch-web.sh logs
  ./scripts/cc-switch-web.sh restart
  ./scripts/cc-switch-web.sh stop

Health check:
  ./scripts/cc-switch-web.sh health

EOF
}

main() {
  log "Starting automatic deployment for $APP_NAME..."
  log "Project directory: $APP_DIR"

  install_system_deps
  install_rust
  install_node
  install_pnpm
  install_frontend_deps
  build_frontend
  build_backend
  start_service
  show_result
}

main "$@"
