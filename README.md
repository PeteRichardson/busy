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

busy asset upload ./horse.anim     # an animation, uploaded untouched
busy draw horse.anim --loop        # the bar plays it on its own clock
busy draw sheet.anim -x -144       # pan a window over an oversized one

busy template init                 # write the example templates
busy draw ok                       # a template with no required variables
busy draw error "Build failed"     # a template that requires a message
git log -1 --format=%s | busy draw error -
```

Every per-invocation flag has a short form; the long form always works too.

| | | | |
|---|---|---|---|
| `-c` `--color` | `-f` `--font` | `-a` `--align` | `-s` `--screen` |
| `-x` `--x` | `-y` `--y` | `-w` `--width` | `-r` `--scroll-rate` |
| `-p` `--priority` | `-t` `--timeout` | `-u` `--until` | `-l` `--led` |
| `-i` `--id` | `-k` `--keep` | `-n` `--dry-run` | `-j` `--json` |
| `-o` `--opacity` | | | |

`-q`/`--quiet` too. One letter is reused: `-y` is `--y` on the commands that
take a position, and `--yes` on `busy asset delete`, which takes none. The two
never appear on the same subcommand. The connection options — `--addr`, `--app`, `--token`,
`--api-prefix`, `--http-timeout` — are deliberately long-only: they are typed
rarely, a global short is reserved across every subcommand, and a short
`--token` would invite secrets into shell history and `ps`. `--var` and
`--template-dir` are long-only for the same reason: they're rarely typed, and
templates are the exception, not the common case. So are `--loop` and
`--section`, which apply only to `.anim` animations.

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
- **Images are fitted to the panel.** The bar decodes PNG but refuses to draw
  anything larger than the display — `display/draw` answers `400 Image …
  exceeds display dimensions 72x16.`, even though the upload itself succeeded.
  So `busy asset upload` scales the image down to fit and tells you when it
  did. Aspect ratio is preserved and an image already small enough is never
  enlarged. JPEG and GIF are converted to PNG on upload (the bar decodes PNG
  only) and stored under a `.png` name. None of this applies to `.anim`
  animations, which are exempt from the size rule — see below.
- **`--screen` on `asset upload` is the fit target, not the destination.** An
  image fitted for the back panel still needs `busy draw --screen back` to be
  drawn there. Drawn on the front, a 160x80 asset exceeds the 72x16 panel and
  the bar rejects it.
- **Animations are uploaded untouched.** A `.anim` is the bar's own animation
  container, and `busy asset upload` recognises one by its signature rather
  than its name — it is stored byte for byte, never re-encoded, and keeps the
  `.anim` suffix, which is what makes `busy draw` treat it as an animation.
  The header is checked first, because the device accepts a malformed
  animation with 200, draws it with 200, and then shows solid magenta: nothing
  in the HTTP conversation admits it failed.
- **`busy draw name.anim` plays it; `--loop` replays it.** Without `--loop`
  the animation stops on its last frame, which is the device's own default.
  `--section NAME` plays one named range of frames — `asset upload` lists the
  names a file offers, since nothing on the device will tell you. Both flags
  are usage errors on anything that is not a `.anim`. Playback runs on the
  firmware's clock: drawing frames yourself is not an option, because every
  `display/draw` takes about 1.5 seconds on the device, capping a draw loop
  near 0.7 fps.
- **An oversized animation is a sprite sheet, not a mistake.** The size limit
  that stops an image dead does not apply to a `.anim`: the firmware cuts a
  panel-sized window out of a larger animation, and `-x`/`-y` move that
  window. `busy draw sheet.anim -x -144` shows the third 72-pixel cell of a
  216-wide sheet, so the off-display warning is suppressed for animations —
  a negative anchor is the instruction there, not a slip. The format stores
  width and height in one byte each, so a sheet caps at 255x255.
- **Assets are all-or-nothing to delete.** The API has no per-file delete, so
  `busy asset delete` removes every asset for the app. It shows the file list
  it is about to destroy, then asks — `--yes` skips the prompt for CI, and
  without it the command refuses outright when stdin isn't a tty rather than
  prompting into the void.
- **`--until` works on `busy text` but not on `busy draw`.** `draw` never
  declares the flag at all, so clap itself rejects it (exit 2) rather than
  the CLI's own usage-error path.
- **`busy draw --file` draws a raw payload file, not a single element.**
  `--priority` and `--led` override the file's own values when given, and
  `--keep` works as usual; `--opacity`, `-x`/`-y`, `--align`, `--screen`,
  `--timeout`, and `--id` are all usage errors, because a payload file owns
  its per-element fields and may hold many elements.
- **Templates.** `busy template init` writes examples into
  `~/.config/busy/templates/`; each is a directory with a `template.toml` that
  is the API payload plus a `description`. `busy draw <name>` renders and draws
  one. Variables are minijinja (`{{ message }}`), the positional binds to
  `message`, and `--var k=v` supplies the rest. Every substitution is escaped,
  so a quote in a commit subject is safe.
- **Templates take flags like `--file` does.** `--priority` and `--led`
  override the template's own values; per-element flags (`-x`, `--align`,
  `--opacity`, `--timeout`, `--id`) are errors, because a template may hold
  several elements. Expose anything else as a `{{ variable }}`.
- **Adding an example.** Commit a directory to `templates/` in this repo and
  `busy template init` picks it up — no code change. `tests/examples.rs`
  validates every one, so a broken template fails the build.
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
