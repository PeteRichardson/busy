#!/bin/sh
# Probe a physical BUSY Bar for behaviour the OpenAPI document does not specify.
#
# Re-run this after a firmware update and diff the output against the findings
# recorded in docs/specs/2026-08-09-busy-cli-ux-design.md §5.
#
#   BUSY_ADDR=http://10.0.4.20/api BUSY_TOKEN=12345678 sh scripts/probe-device.sh
#
# BUSY_TOKEN is only needed when /access is in "key" mode.
#
# Safety: uses a dedicated application_name so that step 6's delete-all cannot
# touch a real app's assets. Do NOT set APP to an app you care about — the only
# delete the API offers is all-or-nothing.

BAR=${BUSY_ADDR:-http://10.0.4.20/api}
APP=${BUSY_PROBE_APP:-busy-probe}
AUTH="Authorization: Bearer $BUSY_TOKEN"
PROBE=$(mktemp -t probe).png

say() { printf '\n=== %s\n' "$1"; }
trap 'rm -f "$PROBE" "$PROBE.readback"' EXIT

# An 8x8 1-bit greyscale PNG, 73 bytes.
printf '%s' \
  'iVBORw0KGgoAAAANSUhEUgAAAAgAAAAIAQAAAADrEsqBAAAAEUlEQVR4nGP4DwUMoxRWCgCTiD/hbEmm5wAAAABJRU5ErkJggg==' \
  | base64 -d > "$PROBE"

say "1. /ext layout — expect apps_assets, user_assets, apps_data, update"
curl -s -H "$AUTH" "$BAR/storage/list?path=/ext"

say "2. upload via the Assets namespace"
curl -s -H "$AUTH" -H 'Content-Type: application/octet-stream' \
  --data-binary "@$PROBE" \
  "$BAR/assets/upload?application_name=$APP&file=probe.png"

say "3. enumerate — expect probe.png, size 73"
curl -s -H "$AUTH" "$BAR/storage/list?path=/ext/user_assets/$APP"

say "4. read back — expect 73 bytes, identical"
curl -s -H "$AUTH" -o "$PROBE.readback" -w 'http %{http_code}, %{size_download} bytes\n' \
  "$BAR/storage/read?path=/ext/user_assets/$APP/probe.png"
cmp -s "$PROBE" "$PROBE.readback" && echo "readback identical" || echo "readback DIFFERS"

say "5. draw by bare name — expect 200"
curl -s -i -H "$AUTH" -H 'Content-Type: application/json' -X POST "$BAR/display/draw" \
  -d "{\"application_name\":\"$APP\",\"priority\":95,
       \"elements\":[{\"id\":\"probe\",\"type\":\"image\",\"path\":\"probe.png\"}]}" \
  | sed -n '1p;/^{/p'

say "6. per-file delete — expect 400, and the file to SURVIVE"
curl -s -H "$AUTH" -X DELETE "$BAR/storage/remove?path=/ext/user_assets/$APP/probe.png"
curl -s -H "$AUTH" "$BAR/storage/list?path=/ext/user_assets/$APP"

say "7. draw a missing asset — expect 400 naming the resolved path"
curl -s -H "$AUTH" -H 'Content-Type: application/json' -X POST "$BAR/display/draw" \
  -d "{\"application_name\":\"$APP\",\"priority\":95,
       \"elements\":[{\"id\":\"probe\",\"type\":\"image\",\"path\":\"does-not-exist.png\"}]}"

say "8. stock assets"
curl -s -H "$AUTH" "$BAR/storage/list?path=/ext/apps_assets/shared/images" | head -c 400; echo

say "9. implicit align — expect the no-align frame to equal top_left"
frame() {
  curl -s -H "$AUTH" -X DELETE "$BAR/display/draw?application_name=$APP" >/dev/null
  curl -s -H "$AUTH" -H 'Content-Type: application/json' -X POST "$BAR/display/draw" \
    -d "{\"application_name\":\"$APP\",\"priority\":95,\"elements\":[{\"id\":\"a\",\"type\":\"text\",\"text\":\"Xy\",\"font\":\"small\",\"x\":0,\"y\":0$1}]}" >/dev/null
  sleep 0.8
  curl -s -H "$AUTH" "$BAR/screen?display=0" | shasum -a 256 | cut -c1-16
}
none=$(frame "")
tl=$(frame ",\"align\":\"top_left\"")
printf '  omitted=%s top_left=%s -> %s\n' "$none" "$tl" \
  "$([ "$none" = "$tl" ] && echo 'MATCH (device default is top_left)' || echo 'CHANGED — the device default is no longer top_left')"
curl -s -H "$AUTH" -X DELETE "$BAR/display/draw?application_name=$APP" >/dev/null

say "10. cleanup — delete-all, then list (expect 400: the directory itself is gone)"
curl -s -H "$AUTH" -X DELETE "$BAR/display/draw?application_name=$APP"; echo
curl -s -H "$AUTH" -X DELETE "$BAR/assets/upload?application_name=$APP"; echo
curl -s -H "$AUTH" "$BAR/storage/list?path=/ext/user_assets/$APP"; echo
