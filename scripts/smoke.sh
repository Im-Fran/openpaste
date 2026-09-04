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

# HTTPS: self-signed cert, round-trip over TLS, and cert-without-key refused
if command -v openssl >/dev/null; then
  openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj /CN=localhost \
    -keyout "$TMP/key.pem" -out "$TMP/cert.pem" >/dev/null 2>&1
  TPORT=$((PORT + 1)); TURL="https://127.0.0.1:$TPORT"; RPORT=$((PORT + 2))
  env -i PATH="$PATH" HOME="$HOME" \
    BASE_URL="$TURL" DATABASE_URL="sqlite://$TMP/tls.sqlite?mode=rwc" \
    STORAGE_DRIVER=local STORAGE_PATH="$TMP/tlsblobs" TLS_RELOAD_SECS=1 \
    ./target/debug/openpaste serve --bind "127.0.0.1:$TPORT" \
    --tls-cert "$TMP/cert.pem" --tls-key "$TMP/key.pem" \
    --http-redirect "127.0.0.1:$RPORT" >"$TMP/tlslog" 2>&1 &
  TPID=$!
  disown $TPID 2>/dev/null || true
  trap 'kill "${PID:-}" "${TPID:-}" 2>/dev/null; rm -rf "$TMP"' EXIT
  for _ in $(seq 50); do curl -skf "$TURL/healthz" >/dev/null 2>&1 && break; sleep 0.1; done
  T=$(printf 'over tls' | curl -skf --data-binary @- "$TURL")
  [ "$(curl -skf "$T/raw")" = "over tls" ] || { cat "$TMP/tlslog"; fail "https round-trip"; }
  # plain HTTP against the TLS port must not answer
  [ "$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$TPORT/healthz")" = 000 ] \
    || fail "TLS port answered plain HTTP"
  env -i PATH="$PATH" HOME="$HOME" TLS_CERT="$TMP/cert.pem" \
    DATABASE_URL="sqlite://$TMP/tls.sqlite?mode=rwc" \
    ./target/debug/openpaste serve --bind "127.0.0.1:$((PORT + 3))" >/dev/null 2>&1 \
    && fail "TLS_CERT without TLS_KEY should refuse to start"
  ok "https paste -> $T"

  # the plain-HTTP listener 308s to BASE_URL, keeping path and query
  R=$(curl -s -o /dev/null -w '%{http_code} %{redirect_url}' "http://127.0.0.1:$RPORT/paste/abc?raw=1")
  [ "$R" = "308 $TURL/paste/abc?raw=1" ] || fail "http->https redirect, got '$R'"
  # and it refuses to start pointing at an http:// BASE_URL (that would loop)
  env -i PATH="$PATH" HOME="$HOME" BASE_URL="http://127.0.0.1:$TPORT" \
    DATABASE_URL="sqlite://$TMP/tls.sqlite?mode=rwc" \
    ./target/debug/openpaste serve --bind "127.0.0.1:$((PORT + 3))" \
    --tls-cert "$TMP/cert.pem" --tls-key "$TMP/key.pem" \
    --http-redirect "127.0.0.1:$((PORT + 4))" >/dev/null 2>&1 \
    && fail "redirect to an http:// BASE_URL should refuse to start"
  ok "http->https redirect"

  # hot reload: swap the cert on disk, the running server must pick it up
  openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj /CN=reloaded.local \
    -keyout "$TMP/key2.pem" -out "$TMP/cert2.pem" >/dev/null 2>&1
  cn() { openssl s_client -connect "127.0.0.1:$TPORT" </dev/null 2>/dev/null | grep -m1 -o 'CN *= *[^,]*'; }
  cn | grep -q localhost || fail "unexpected certificate before the reload"
  mv "$TMP/cert2.pem" "$TMP/cert.pem"; mv "$TMP/key2.pem" "$TMP/key.pem"
  for _ in $(seq 40); do cn | grep -q reloaded.local && break; sleep 0.25; done
  cn | grep -q reloaded.local || { cat "$TMP/tlslog"; fail "certificate hot reload"; }
  [ "$(curl -skf "$T/raw")" = "over tls" ] || fail "https round-trip after the reload"
  kill "$TPID" 2>/dev/null || true
  ok "certificate hot reload"
else
  echo "skip - https (no openssl)"
fi

echo "all good"
