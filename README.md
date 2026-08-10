# busy

An ergonomic CLI for the [BUSY Bar](https://busy.app).

```sh
busy text "Hello, World!"
busy text -x 0 -y 8 --align mid_left --font small --color red "Goodbye!"
busy text --timeout 30 --priority urgent "deploy done"
git log -1 --format=%s | busy text -
busy clear
```

## Install

```sh
cargo install --path .
```

## Configuration

Highest precedence wins: CLI flags, then environment (`BUSY_ADDR`, `BUSY_TOKEN`,
`BUSY_APP`), then `~/.config/busy/config.toml`, then built-in defaults.

```toml
addr = "http://10.0.4.20"
app  = "busy"

[defaults]
font     = "large"
align    = "center"
color    = "#ffffffff"
priority = 95
```

With no `-x`/`-y`, an element is anchored to the centre of the selected
display — `(36, 8)` on the 72x16 front panel, `(80, 40)` on the 160x80 back —
which is why `busy text "hi"` looks deliberately centred rather than merely
correct.

Keep the access key out of `argv` — prefer `BUSY_TOKEN` or the config file so it
stays out of your shell history and out of `ps`.

## Notes

- **Priority.** A draw is accepted only when its priority is at least the running
  app's. Built-in apps run at 10; an active BUSY work session runs at 90. The
  CLI defaults to 95 so a deliberate notification wins. `--priority` also accepts
  `low`, `normal`, `high`, `urgent` (10, 50, 95, 100).
- **Replace by default, and it flickers.** `POST display/draw` upserts by
  element id and never removes, so `busy` clears its own elements before
  drawing — that's a `DELETE` followed by a `POST`, and the panel measurably
  blanks between the two. Pass `--keep` to compose onto what is already on
  screen instead, for scripts that cannot tolerate the blink.
- **Font size and overflow.** A `--font large` message fits roughly 13
  characters before the front panel's 72px width clips it silently; `small`
  fits about 22. The CLI estimates the rendered width and warns when it looks
  like it will overflow, pointing at `--width`/`--scroll-rate`.
- **ASCII only.** The bar's fonts are bitmap ASCII. Smart quotes, dashes, and
  ellipses are transliterated automatically and a warning is printed; anything
  else is dropped.
- `--dry-run` prints the exact JSON that would be sent and contacts nothing.
- `busy text -- -3 tests failing` reaches a message starting with `-` (the
  `--` just stops clap from treating it as a flag); a bare `-` after `--`
  still reads stdin rather than producing a literal `-`.

## Prior art

[`busybar-rust`](https://github.com/foresterre/busybar-rust) provides `busylib`,
which this tool is built on, and a `busybar` CLI that mirrors the API 1:1. For
frame capture and screen mirroring, use that.
