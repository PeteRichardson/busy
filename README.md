# busy

An ergonomic CLI for the [BUSY Bar](https://busy.app) , built on
[`busylib`](https://github.com/foresterre/busybar-rust) by [@foresterre](https://github.com/foresterre).

```sh
busy text "Hello, World!"
busy text -x 0 -y 8 -a mid_left -f small -c red "Goodbye!"
busy text -t 30 -p urgent "deploy done"
git log -1 --format=%s | busy text -
busy clear

busy asset upload ./logo.png       # fit for the front panel, stored as logo.png
busy draw logo.png                 # draw it
busy draw shared/checkmark_front_8x8.image
busy asset list
```

Every per-invocation flag has a short form; the long form always works too.

| | | | |
|---|---|---|---|
| `-c` `--color` | `-f` `--font` | `-a` `--align` | `-s` `--screen` |
| `-x` `--x` | `-y` `--y` | `-w` `--width` | `-r` `--scroll-rate` |
| `-p` `--priority` | `-t` `--timeout` | `-u` `--until` | `-l` `--led` |
| `-i` `--id` | `-k` `--keep` | `-n` `--dry-run` | `-j` `--json` |
| `-o` `--opacity` | | | |

`-q`/`--quiet` too. The connection options — `--addr`, `--app`, `--token`,
`--api-prefix`, `--http-timeout` — are deliberately long-only: they are typed
rarely, a global short is reserved across every subcommand, and a short
`--token` would invite secrets into shell history and `ps`.

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
  fits about 22. The CLI estimates the rendered width and warns when the text
  is wider than the panel, pointing at `--width`/`--scroll-rate`. It does not
  currently catch the narrower case of short text positioned so close to an
  edge that most of it falls off.
- **ASCII only.** The bar's fonts are bitmap ASCII. Smart quotes, dashes, and
  ellipses are transliterated automatically and a warning is printed; anything
  else is dropped.
- **Images are fitted, not cropped.** The bar decodes PNG and silently crops
  anything larger than the panel, so `busy asset upload` scales the image down
  to fit and tells you when it did. JPEG and GIF are converted to PNG on upload
  (the bar decodes PNG only) and stored under a `.png` name.
- **`--screen` on `asset upload` is the fit target, not the destination.** An
  image fitted for the back panel still needs `busy draw --screen back` to be
  drawn there; drawn on the front, the bar will crop it.
- **Assets are all-or-nothing to delete.** The API has no per-file delete, so
  `busy asset delete` removes every asset for the app. It shows the file list
  it is about to destroy, then asks — `--yes` skips the prompt for CI, and
  without it the command refuses outright when stdin isn't a tty rather than
  prompting into the void.
- **`--until` works on `busy text` but not on `busy draw`.** It's a usage
  error there (exit 2), pointing at `--timeout`.
- **`busy draw --file` draws a raw payload file, not a single element.**
  `--priority` and `--led` override the file's own values when given, and
  `--keep` works as usual; `--opacity`, `-x`/`-y`, `--align`, `--screen`,
  `--timeout`, `--id`, and `--until` are all usage errors, because a payload
  file owns its per-element fields and may hold many elements.
- `--dry-run` prints the exact JSON that would be sent and never changes
  anything on the device — read-only calls (like the file listing
  `busy asset delete --dry-run` makes so it can name what it would destroy)
  are allowed; mutations are not.
- `busy text -- -3 tests failing` reaches a message starting with `-` (the
  `--` just stops clap from treating it as a flag); a bare `-` after `--`
  still reads stdin rather than producing a literal `-`.

## Acknowledgements

`busy` is a thin ergonomic layer over
[`busylib`](https://github.com/foresterre/busybar-rust) by
[@foresterre](https://github.com/foresterre). It does the real work of talking
to the bar — this project is mostly argument parsing and opinions on top of it.

The same repo ships a `busybar` CLI that mirrors the HTTP API 1:1. If you want
frame capture or screen mirroring, or you'd rather have the full API surface
than defaults chosen for you, use that instead.
