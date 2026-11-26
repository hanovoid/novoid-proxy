#!/bin/sh
set -e

APP_DIR="$(cd "$(dirname "$0")" && pwd)"
ROUTER_PORT="${PORT:-8080}"
CORE_PORT="${NOVOID_CORE_PORT:-8081}"

if [ "$ROUTER_PORT" = "$CORE_PORT" ]; then
  CORE_PORT="8082"
fi

# Tim binary novoid-core (PATH, /usr/local/bin, hoac build local)
CORE_BIN="${NOVOID_CORE_BIN:-}"
if [ -z "$CORE_BIN" ]; then
  if command -v novoid-core >/dev/null 2>&1; then
    CORE_BIN="$(command -v novoid-core)"
  elif [ -x "/usr/local/bin/novoid-core" ]; then
    CORE_BIN="/usr/local/bin/novoid-core"
  elif [ -x "$APP_DIR/../novoid-core/target/release/novoid-core" ]; then
    CORE_BIN="$APP_DIR/../novoid-core/target/release/novoid-core"
  elif [ -x "$APP_DIR/../novoid-core/target/debug/novoid-core" ]; then
    CORE_BIN="$APP_DIR/../novoid-core/target/debug/novoid-core"
  fi
fi

if [ -z "$CORE_BIN" ] || [ ! -x "$CORE_BIN" ]; then
  echo "[NovoidProxy] Khong tim thay binary novoid-core. Hay dat NOVOID_CORE_BIN hoac build Docker image." >&2
  exit 1
fi

# Tim binary novoid-proxy
PROXY_BIN="${NOVOID_PROXY_BIN:-}"
if [ -z "$PROXY_BIN" ]; then
  if command -v novoid-proxy >/dev/null 2>&1; then
    PROXY_BIN="$(command -v novoid-proxy)"
  elif [ -x "/usr/local/bin/novoid-proxy" ]; then
    PROXY_BIN="/usr/local/bin/novoid-proxy"
  elif [ -x "$APP_DIR/target/release/novoid-proxy" ]; then
    PROXY_BIN="$APP_DIR/target/release/novoid-proxy"
  elif [ -x "$APP_DIR/target/debug/novoid-proxy" ]; then
    PROXY_BIN="$APP_DIR/target/debug/novoid-proxy"
  fi
fi

if [ -z "$PROXY_BIN" ] || [ ! -x "$PROXY_BIN" ]; then
  echo "[NovoidProxy] Khong tim thay binary novoid-proxy. Hay chay 'cargo build --release' truoc." >&2
  exit 1
fi

echo "[NovoidProxy] Dang khoi chay novoid-core tai cong $CORE_PORT..."
if [ "${NOVOID_CORE_FALLBACK:-false}" = "true" ]; then
  HOST="127.0.0.1" PORT="$CORE_PORT" "$CORE_BIN" serve --fallback &
else
  HOST="127.0.0.1" PORT="$CORE_PORT" "$CORE_BIN" serve &
fi
CORE_PID=$!

ROUTER_PID=""
cleanup() {
  if [ -n "$ROUTER_PID" ] && kill -0 "$ROUTER_PID" 2>/dev/null; then
    kill -TERM "$ROUTER_PID" 2>/dev/null || true
  fi
  if kill -0 "$CORE_PID" 2>/dev/null; then
    kill -TERM "$CORE_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

echo "[NovoidProxy] Dang cho novoid-core (cong $CORE_PORT) san sang..."
READY=0
for i in $(seq 1 30); do
  if curl -sf "http://127.0.0.1:${CORE_PORT}/health" >/dev/null 2>&1 || curl -sf "http://127.0.0.1:${CORE_PORT}/v1/models" >/dev/null 2>&1; then
    READY=1
    break
  fi
  if ! kill -0 "$CORE_PID" 2>/dev/null; then
    echo "[NovoidProxy] Loi: novoid-core da dung dot ngot." >&2
    exit 1
  fi
  sleep 1
done

if [ "$READY" -ne 1 ]; then
  echo "[NovoidProxy] Canh bao: novoid-core chua phan hoi sau 30s, van tiep tuc khoi chay router..." >&2
fi

HOST="${HOST:-0.0.0.0}" PORT="$ROUTER_PORT" UPSTREAM_URL="http://127.0.0.1:${CORE_PORT}" "$PROXY_BIN" &
ROUTER_PID=$!
wait "$ROUTER_PID"
