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
trap 'rm -f "$PROBE" "$PROBE.readback" "$PROBE.big" "$PROBE.jpg" "$PROBE.drawresp" "$PROBE.frame"' EXIT

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

say "10. oversized images — expect a 200 and a CROPPED render, not a scaled one"
# A 200x100 canvas: a solid-red 72x16 rectangle filling the exact top-left
# corner, blue everywhere else. Genuinely oversized in both dimensions —
# design doc §1 measured crop behaviour with a 200x100 source. If the
# device crops from the top-left with no scaling, the whole visible screen
# reads back red; if it scales instead, red shrinks to a small corner and
# blue dominates.
#
# Verified before embedding, three independent ways: `file` and
# `sips -g pixelWidth -g pixelHeight` both report "200 x 100"; a
# hand-decoded IHDR gives width=200 height=100 bit-depth=8 colour-type=2
# (RGB); and sampling the decompressed IDAT confirms the red rectangle
# occupies exactly (0,0)-(71,15) with blue everywhere else.
printf '%s' \
  'iVBORw0KGgoAAAANSUhEUgAAAMgAAABkCAIAAABM5OhcAAAA2klEQVR42u3SAQ0AAAgCQfqX1hzobVcA9pnkpKOzeggLYQlLWMISlrAQlrCEJSxhCQthCUtYwhKWsBCWsIQlLGEJC2EJS1jC+sgFCAthISwQFsJCWCAshIWwQFgIC2GBsBAWwgJhISyEBcJCWAgLhIWwEBYIC2EhLBAWwkJYICyEhbBAWAgLYYGwEBbCAmEhLIQFwkJYCAuEhbAQFggLYSEsEBbCQlggLISFsEBYCAthgbAQFsICYSEshAXCQlgIC4SFsBAWCAthISwQFsJCWCAshIWwQFgIiyYLH6vWZGiyKzkAAAAASUVORK5CYII=' \
  | base64 -d > "$PROBE.big"
curl -s -H "$AUTH" -H 'Content-Type: application/octet-stream' --data-binary "@$PROBE.big" \
  "$BAR/assets/upload?application_name=$APP&file=big.png" ; echo
curl -s -H "$AUTH" -X DELETE "$BAR/display/draw?application_name=$APP" >/dev/null
draw_code=$(curl -s -H "$AUTH" -H 'Content-Type: application/json' -X POST "$BAR/display/draw" \
  -o "$PROBE.drawresp" -w '%{http_code}' \
  -d "{\"application_name\":\"$APP\",\"priority\":95,\"elements\":[{\"id\":\"b\",\"type\":\"image\",\"path\":\"big.png\",\"x\":0,\"y\":0,\"align\":\"top_left\"}]}")
if [ "$draw_code" != 200 ]; then
  printf '  draw: expect 200, got %s -> CHANGED (design doc §1 measured 200 + a silent crop here)\n' "$draw_code"
  printf '  body: %s\n' "$(cat "$PROBE.drawresp")"
else
  sleep 0.8
  curl -s -H "$AUTH" "$BAR/screen?display=0" | base64 -d > "$PROBE.frame"
  # /screen is BGR888 for the front panel — measured directly: a solid
  # (10, 200, 50) RGB source reads back as (50, 200, 10) bytes. Step 9's
  # monochrome text probes never exposed this because black/white is
  # channel-order independent.
  od -An -tu1 -v "$PROBE.frame" | tr -s ' ' '\n' | grep -v '^$' | awk '
    { bytes[NR - 1] = $1 }
    END {
      w = 72; h = 16
      red = 0; blue = 0; other = 0; total = w * h
      for (y = 0; y < h; y++) {
        line = "  "
        for (x = 0; x < w; x++) {
          i = (y * w + x) * 3
          b = bytes[i]; g = bytes[i + 1]; r = bytes[i + 2]
          if (r > 150 && g < 100 && b < 100) { c = "R"; red++ }
          else if (b > 150 && r < 100 && g < 100) { c = "B"; blue++ }
          else { c = "?"; other++ }
          line = line c
        }
        print line
      }
      printf "  red=%d blue=%d other=%d (of %d)\n", red, blue, other, total
      if (red * 2 > total) {
        print "  CROPPED (as measured)"
      } else {
        print "  CHANGED -- the device no longer crops this the way design doc §1 measured"
      }
    }
  '
fi

say "11. JPEG — expect upload 200 and draw 400 (the device decodes PNG only)"
if command -v sips >/dev/null 2>&1 && sips -s format jpeg "$PROBE" --out "$PROBE.jpg" >/dev/null 2>&1; then
  upload_code=$(curl -s -H "$AUTH" -H 'Content-Type: application/octet-stream' --data-binary "@$PROBE.jpg" \
    -o /dev/null -w '%{http_code}' \
    "$BAR/assets/upload?application_name=$APP&file=probe.jpg")
  draw_code=$(curl -s -H "$AUTH" -H 'Content-Type: application/json' -X POST "$BAR/display/draw" \
    -o /dev/null -w '%{http_code}' \
    -d "{\"application_name\":\"$APP\",\"priority\":95,\"elements\":[{\"id\":\"j\",\"type\":\"image\",\"path\":\"probe.jpg\"}]}")
  printf '  upload: expect 200, got %s -> %s\n' "$upload_code" \
    "$([ "$upload_code" = 200 ] && echo MATCH || echo CHANGED)"
  printf '  draw:   expect 400, got %s -> %s\n' "$draw_code" \
    "$([ "$draw_code" = 400 ] && echo MATCH || echo CHANGED)"
else
  echo "  (sips unavailable, skipping)"
fi

say "12. cleanup — delete-all, then list (expect 400: the directory itself is gone)"
curl -s -H "$AUTH" -X DELETE "$BAR/display/draw?application_name=$APP"; echo
curl -s -H "$AUTH" -X DELETE "$BAR/assets/upload?application_name=$APP"; echo
curl -s -H "$AUTH" "$BAR/storage/list?path=/ext/user_assets/$APP"; echo
