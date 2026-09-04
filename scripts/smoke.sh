#!/usr/bin/env bash
# End-to-end check: text paste, binary paste, raw, download, size limit.
set -euo pipefail
cd "$(dirname "$0")/.."

TMP=$(mktemp -d); trap 'kill "${PID:-}" 2>/dev/null; rm -rf "$TMP"' EXIT
PORT=${PORT:-18080}
URL="http://127.0.0.1:$PORT"

cargo build --quiet
env -i PATH="$PATH" HOME="$HOME" \
  BIND="127.0.0.1:$PORT" BASE_URL="$URL" \
  DATABASE_URL="sqlite://$TMP/db.sqlite?mode=rwc" \
  STORAGE_DRIVER=local STORAGE_PATH="$TMP/blobs" \
  MAX_UPLOAD_BYTES=1048576 \
  ./target/debug/openpaste serve >"$TMP/log" 2>&1 &
PID=$!
disown $PID 2>/dev/null || true
for _ in $(seq 50); do curl -sf "$URL/api/pastes/nope" >/dev/null 2>&1 && break; sleep 0.1; done

ok() { echo "ok - $1"; }
fail() { echo "FAIL - $1"; cat "$TMP/log"; exit 1; }

# text round-trip
P=$(printf 'hello\nworld\n' | curl -sf --data-binary @- "$URL")
[ "$(curl -sf "$P/raw")" = "$(printf 'hello\nworld')" ] || fail "text raw round-trip"
ok "text paste -> $P"

# binary round-trip: content preserved byte for byte
head -c 4096 /dev/urandom > "$TMP/blob.bin"
B=$(curl -sf -T "$TMP/blob.bin" "$URL/blob.bin")
curl -sf "$B/download" -o "$TMP/out.bin"
cmp "$TMP/blob.bin" "$TMP/out.bin" || fail "binary round-trip"
curl -sfI "$B/download" | grep -qi 'filename="blob.bin"' || fail "download filename"
ok "binary paste -> $B"

# unknown id is 404, oversized upload is rejected
[ "$(curl -s -o /dev/null -w '%{http_code}' "$URL/paste/zzzzzzzz/raw")" = 404 ] || fail "404 on unknown id"
head -c 2000000 /dev/zero | tr '\0' 'a' > "$TMP/big.txt"
[ "$(curl -s -o /dev/null -w '%{http_code}' --data-binary @"$TMP/big.txt" "$URL")" = 413 ] || fail "size limit"
ok "404 + size limit"

# web UI: assets are reachable and the htmx form redirects to the new paste
[ "$(curl -s -o /dev/null -w '%{http_code}' "$URL/style.css")" = 200 ] || fail "asset /style.css"
curl -sf "$URL" | grep -q 'hx-post="/ui/new"' || fail "index has the htmx form"
H=$(curl -sf -D - -o /dev/null -F 'content=from the form' "$URL/ui/new" | tr -d '\r' | grep -i '^hx-redirect:' | awk '{print $2}')
[ "$(curl -sf "$URL$H/raw")" = "from the form" ] || fail "form -> hx-redirect -> paste"
curl -sf "$URL$H" | grep -q '<pre id="content">from the form</pre>' || fail "server-rendered paste view"
ok "web ui -> $H"

# CLI round-trip
[ "$(echo 'via cli' | ./target/debug/openpaste up --server "$URL" | xargs -I{} ./target/debug/openpaste get {})" = "via cli" ] || fail "cli up/get"
ok "cli up | get"

echo "all good"
