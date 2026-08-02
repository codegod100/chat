#!/usr/bin/env bash
# Publish chat.apk to a public https://chat-apk.boxd.sh URL.
#   ./scripts/publish-apk.sh [path/to/chat.apk]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APK="${1:-$ROOT/android/target/release/apk/chat.apk}"
VM="${CHAT_APK_VM:-chat-apk}"
PORT=8000
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

[[ -f "$APK" ]] || {
  echo "APK not found: $APK" >&2
  echo "  build first: ./scripts/build-apk.sh --release" >&2
  exit 1
}

need() { command -v "$1" >/dev/null || { echo "missing: $1" >&2; exit 1; }; }
need boxd
need curl

cp -f "$APK" "$STAGE/chat.apk"
cat >"$STAGE/index.html" <<'HTML'
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Chat APK</title>
<style>
  body { font-family: system-ui, sans-serif; max-width: 36rem; margin: 3rem auto; padding: 0 1.25rem; line-height: 1.5; color: #1a1a1a; }
  a.btn { display: inline-block; margin-top: 1rem; padding: 0.75rem 1.25rem; background: #3584e4; color: #fff; text-decoration: none; border-radius: 8px; font-weight: 600; }
  code { background: #f0f0f0; padding: 0.1em 0.35em; border-radius: 4px; }
  .muted { color: #666; font-size: 0.95rem; }
</style>
</head>
<body>
  <h1>Chat</h1>
  <p>Android package <code>uk.nandi.chat</code> (aarch64 release).</p>
  <p class="muted">Vidya LLM chat — OpenBao keys. Paste a token in-app if none is on the device.</p>
  <a class="btn" href="/chat.apk">Download chat.apk</a>
</body>
</html>
HTML

if ! boxd info "$VM" >/dev/null 2>&1; then
  echo "creating VM $VM…" >&2
  boxd new --name="$VM" --auto-suspend-timeout=0
fi

boxd proxy set-port --vm="$VM" --port="$PORT" >/dev/null || true

boxd exec "$VM" -- mkdir -p www .config/systemd/user
boxd cp "$STAGE/chat.apk" "$VM:www/chat.apk"
boxd cp "$STAGE/index.html" "$VM:www/index.html"

# Persistent user service (survives boxd exec exit; nohup does not reliably).
boxd exec "$VM" -- bash -c "
set -euo pipefail
cat > \"\$HOME/.config/systemd/user/chat-apk-http.service\" <<'EOF'
[Unit]
Description=Chat APK static file server
After=network.target

[Service]
WorkingDirectory=%h/www
ExecStart=/usr/bin/python3 -m http.server ${PORT} --bind 0.0.0.0
Restart=always

[Install]
WantedBy=default.target
EOF
systemctl --user daemon-reload
systemctl --user enable --now chat-apk-http.service
sleep 0.5
systemctl --user is-active chat-apk-http.service
ss -tln | grep -q ':${PORT}' || { echo 'server not listening :${PORT}' >&2; exit 1; }
"

URL="https://${VM}.boxd.sh"
echo "published:" >&2
echo "  $URL/" >&2
echo "  $URL/chat.apk" >&2
sleep 1
curl -sI "$URL/chat.apk" | head -8 >&2 || true
echo "$URL/chat.apk"
