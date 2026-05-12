#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

BIN="${CC_SWITCH_BIN:-$APP_DIR/src-tauri/target/release/cc-switch}"
DIST="${CC_SWITCH_WEB_DIST:-$APP_DIR/dist}"
BIND="${CC_SWITCH_WEB_BIND:-[::]:3001,0.0.0.0:3001}"
LOG_FILE="${CC_SWITCH_WEB_LOG:-$APP_DIR/cc-switch-web.log}"
PID_FILE="${CC_SWITCH_WEB_PID:-/tmp/cc-switch-web.pid}"

usage() {
  cat <<EOF
Usage: $0 [run|start|stop|restart|status|logs|health]

Commands:
  run       Run in foreground
  start     Run in background with nohup
  stop      Stop background process
  restart   Restart background process
  status    Show process status
  logs      Follow log file
  health    Check /api/health

Environment overrides:
  CC_SWITCH_WEB_BIND   Default: $BIND
  CC_SWITCH_WEB_DIST   Default: $DIST
  CC_SWITCH_BIN        Default: $BIN
  CC_SWITCH_WEB_LOG    Default: $LOG_FILE
  CC_SWITCH_WEB_PID    Default: $PID_FILE
EOF
}

ensure_ready() {
  if [[ ! -x "$BIN" ]]; then
    echo "Executable not found: $BIN"
    echo "Build it first:"
    echo "  cd $APP_DIR/src-tauri && cargo build --release"
    exit 1
  fi

  if [[ ! -d "$DIST" || ! -f "$DIST/index.html" ]]; then
    echo "Frontend dist not found: $DIST"
    echo "Build it first:"
    echo "  cd $APP_DIR && pnpm build:renderer"
    exit 1
  fi
}

pid_is_running() {
  [[ -f "$PID_FILE" ]] && kill -0 "$(cat "$PID_FILE")" >/dev/null 2>&1
}

run_foreground() {
  ensure_ready
  echo "Starting CC Switch Web on http://$BIND"
  echo "Static files: $DIST"
  exec env \
    CC_SWITCH_WEB=1 \
    CC_SWITCH_WEB_BIND="$BIND" \
    CC_SWITCH_WEB_DIST="$DIST" \
    "$BIN"
}

start_background() {
  ensure_ready

  if pid_is_running; then
    echo "CC Switch Web is already running, pid=$(cat "$PID_FILE")"
    exit 0
  fi

  mkdir -p "$(dirname "$LOG_FILE")" "$(dirname "$PID_FILE")"

  nohup env \
    CC_SWITCH_WEB=1 \
    CC_SWITCH_WEB_BIND="$BIND" \
    CC_SWITCH_WEB_DIST="$DIST" \
    "$BIN" \
    >"$LOG_FILE" 2>&1 &

  echo $! >"$PID_FILE"
  echo "CC Switch Web started, pid=$(cat "$PID_FILE")"
  echo "URL: http://$BIND"
  echo "Log: $LOG_FILE"
}

stop_background() {
  if ! [[ -f "$PID_FILE" ]]; then
    echo "PID file not found: $PID_FILE"
    exit 0
  fi

  local pid
  pid="$(cat "$PID_FILE")"

  if kill -0 "$pid" >/dev/null 2>&1; then
    echo "Stopping CC Switch Web, pid=$pid"
    kill "$pid"
    for _ in $(seq 1 20); do
      if ! kill -0 "$pid" >/dev/null 2>&1; then
        rm -f "$PID_FILE"
        echo "Stopped"
        return
      fi
      sleep 0.2
    done
    echo "Process did not stop in time, sending SIGKILL"
    kill -9 "$pid" >/dev/null 2>&1 || true
  fi

  rm -f "$PID_FILE"
  echo "Stopped"
}

show_status() {
  if pid_is_running; then
    echo "CC Switch Web is running, pid=$(cat "$PID_FILE")"
    echo "URL: http://$BIND"
    echo "Log: $LOG_FILE"
  else
    echo "CC Switch Web is not running"
    [[ -f "$PID_FILE" ]] && echo "Stale PID file: $PID_FILE"
  fi
}

show_logs() {
  touch "$LOG_FILE"
  tail -f "$LOG_FILE"
}

check_health() {
  local host="$BIND"
  if [[ "$host" == *"0.0.0.0:"* ]]; then
    local ipv4_part="${host#*0.0.0.0:}"
    local port="${ipv4_part%%[^0-9]*}"
    host="127.0.0.1:$port"
  elif [[ "$host" == "[::]:"* ]]; then
    host="[::1]:${host##*:}"
  elif [[ "$host" == "0.0.0.0:"* ]]; then
    host="127.0.0.1:${host##*:}"
  fi
  curl -fsS "http://$host/api/health"
  echo
}

cmd="${1:-run}"

case "$cmd" in
  run)
    run_foreground
    ;;
  start)
    start_background
    ;;
  stop)
    stop_background
    ;;
  restart)
    stop_background
    start_background
    ;;
  status)
    show_status
    ;;
  logs)
    show_logs
    ;;
  health)
    check_health
    ;;
  -h|--help|help)
    usage
    ;;
  *)
    echo "Unknown command: $cmd"
    usage
    exit 1
    ;;
esac
