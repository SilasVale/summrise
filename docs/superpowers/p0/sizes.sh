#!/usr/bin/env bash
# Byte-exact sizes. `stat -c%s` for raw, `gzip -9 -c | wc -c` for gzipped, and a REAL
# loopback fetch with `curl -w '%{size_download}'` as an independent cross-check of the raw count.
# The server binds 127.0.0.1 ONLY (the standing rule on this box).
set -uo pipefail
DIR="$1"; shift
PORT="$1"; shift
cd "$DIR"
python3 -m http.server --bind 127.0.0.1 "$PORT" --directory "$DIR" >/dev/null 2>&1 &
SRV=$!
trap 'kill $SRV 2>/dev/null' EXIT
sleep 1.2
RAW_TOTAL=0; GZ_TOTAL=0; CURL_TOTAL=0
printf '%-42s %10s %10s %10s\n' FILE RAW GZIP CURL
for f in "$@"; do
  raw=$(stat -c%s "$DIR/$f")
  gz=$(gzip -9 -c "$DIR/$f" | wc -c)
  curl_b=$(curl -s -o /dev/null -w '%{size_download}' "http://127.0.0.1:$PORT/$f")
  RAW_TOTAL=$((RAW_TOTAL+raw)); GZ_TOTAL=$((GZ_TOTAL+gz)); CURL_TOTAL=$((CURL_TOTAL+curl_b))
  printf '%-42s %10s %10s %10s\n' "$f" "$raw" "$gz" "$curl_b"
done
printf '%-42s %10s %10s %10s\n' "TOTAL" "$RAW_TOTAL" "$GZ_TOTAL" "$CURL_TOTAL"
