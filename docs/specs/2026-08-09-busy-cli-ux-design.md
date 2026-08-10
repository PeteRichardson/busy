# `busy` CLI — command surface design

**Design spec, 2026-08-09**

A revision of the target UX in `docs/busy-cli-architecture.md` §3, §4, §6.1, §6.2, §7,
§8 and §9. Everything not restated here — priority handling, lifetime flags, colour
parsing, ASCII sanitization, config precedence, the `device.rs` adapter policy —
carries over from that document unchanged.

The architecture doc is the authority on *what the device does*. This document is the
authority on *what the user types*.

---

## 1. The problem

The original surface made the message a flag:

```sh
busy -m "Hello, World!"
```

`-m` is three keystrokes of ceremony on the command typed most often during
development, so the obvious simplification is a bare positional: `busy "Hello, World!"`.

**That doesn't work, and the reason generalises.** clap resolves subcommands before
positionals, and the shell has already stripped the quotes. With `text`, `image`,
`clear`, `asset`, `template`, `status` and `help` as subcommands:

```sh
busy "clear"     # wipes the display
busy "status"    # prints device status
busy "$MSG"      # depends on what $MSG is today
```

In the stated primary use — CI and agentic workflows — the message is a variable. A
notification whose text happens to be `clear` silently performs a different operation.
That is precisely the failure class the architecture doc's §5 exists to eliminate.

The same reasoning recurs at every level of the design, so state it once as a rule:

> **Never discriminate between namespaces by the *shape* of a token.** Discriminate by
> an explicit keyword, a flag, or a *lookup* whose result the user can inspect.

Shape rules ("has a slash, so it's a path", "looks like a filename, so it's an asset")
fail silently and are invisible in `--help`. Lookup rules ("is there a template
directory by this name?") are checkable with `busy template list` and visible in
`--dry-run`.

The user's other worry — that a positional breaks down when a command needs two text
elements — is not a real constraint. Multi-element layouts are templates. If the CLI
ever needs a second string it will arrive as `--var`, not as a second positional.

---

## 2. The command surface

```sh
# text
busy text "Hello, World!"
busy text -                                    # message from stdin
busy text -x 0 -y 8 --align mid_left --font small --color 0xFF0000FF "Goodbye!"

# draw a named thing
busy draw error "Build Failed!"                # a template
busy draw build-complete                       # a template that takes no variables
busy draw stop.png --opacity 50                # an uploaded asset
busy draw shared/clock.png                     # a device built-in
busy draw --file payload.json                  # raw DisplayElements

# templates
busy template list
busy template show error
busy template validate error
busy template run error "Build Failed!"

# assets
busy asset upload ./stop.png
busy asset list
busy asset delete

# device
busy clear
busy status
```

`busy` with no subcommand prints help and exits non-zero.

### 2.1 No implicit default subcommand

The architecture doc's §6.1 used `args_conflicts_with_subcommands = true` with a
flattened `TextArgs` so that `busy -m "hi"` parsed with no subcommand. **Drop it.**

Its only remaining value was saving two keystrokes over `busy text "hi"`, and it cost
an all-`Option` `TextArgs` whose emptiness had to be caught at runtime, a `--help` page
that advertised a phantom command, and one more clap edge case around
subcommand/positional interaction. Requiring the subcommand lets clap produce the
"missing required argument" error for free.

### 2.2 No `-m` / `--message`

The flag existed to make the implicit default parse. With the default gone, it has no
job: `text` and `draw` both take the message as a positional. Keeping it would mean two
spellings of one thing, forever, with no functional gain.

Nothing has shipped, so there is no compatibility argument for retaining it.

### 2.3 Positional arity and stdin

`text` takes a single `String` positional, not a `Vec<String>` joined on spaces.
Joining would silently paper over shell-quoting mistakes, which is the wrong trade for
a tool whose input is usually machine-generated.

`-` as the positional reads the message from stdin. This is the conventional sentinel
and upstream `busybar assets draw --file -` already uses it, so it will not surprise
anyone. It matters because in CI the message is usually already in a pipe:

```sh
git log -1 --format=%s | busy text -
```

`text` requires a message: no positional and no stdin is an error, because `Text` is
`minLength: 1`. `busy text ""` sanitizes to empty and hits the clear-error path the
architecture doc's §8 already specifies.

`-` also works as the *message* positional of `draw` and `template run`
(`… | busy draw error -`), where it binds to the `message` variable exactly as a literal
argument would. It is never a template or asset name.

**Known limitation, verified rather than assumed:** a message consisting of exactly `-`
cannot be expressed. `busy text -- -` does not help — `--` only stops clap treating the
value as a flag, and the resolved string reaching the message resolver is still `"-"`,
which is indistinguishable from the sentinel. Any *longer* message beginning with `-`
is fine and needs only `--` (`busy text -- "-3 tests failing"`). A one-character `-`
notification is not worth a second escape convention, so this is accepted rather than
fixed; it is recorded here because an earlier draft of this document wrongly claimed
`busy text -- -` worked.

---

## 3. `draw`: one verb for "put this on the bar"

The architecture doc had a separate `busy image` subcommand. It is deleted, and
`cmd/image.rs` is folded into `cmd/draw.rs`.

**The unifying idea: `draw` takes a name that expands to `DisplayElements`.** A template
expands to several elements; a bare asset expands to a single `ImageElement`. Same verb,
same overrides, same delivery semantics.

This is consistent with what the architecture doc already specified rather than a new
concept. §7's pipeline is `load template → substitute vars → apply CLI overrides → sync
assets → validate → draw`, and §6.2 has `text` and `template run` flattening the same
arg structs — so template invocation *already* accepted the full Style/Placement/
Delivery surface as overrides. Folding images in adds `--opacity` to that surface and
nothing else. `--file payload.json` falls out for free and gives parity with upstream
`busybar assets draw --file`.

### 3.1 Resolution order

For `busy draw <name>`:

1. `<name>` begins with `shared/` → `stock_path`.
2. A template directory `<name>` exists under the template path → template.
3. Otherwise → an app asset path → a single `ImageElement`.

Rule 1 is not a shape heuristic: `shared/` is the spec's own reserved namespace, fixed
by `stock_path`'s pattern `shared/[a-z0-9_.]+$`. Rule 2 is a filesystem lookup — cheap,
offline, and enumerable by `busy template list`.

The ambiguity rule 1 resolves is real: the app-asset `path` pattern
(`^[a-zA-Z0-9._/-]+$`) also matches `shared/clock.png`, so without an explicit rule that
input has two valid readings.

`busy draw` with neither a name nor `--file` is an error; the two are mutually
exclusive, enforced with a clap arg group.

**Escape hatches.** `--as template|image|stock` forces the interpretation for
pathological cases (a template directory named `logo.png`, an uploaded asset named
`error`). `--dry-run` prints the resolved payload, so the outcome is always inspectable
before it reaches the device.

### 3.2 Making misresolution loud

The residual risk is a typo'd template name falling through to rule 3 and becoming a
doomed asset draw. Two guards:

- **Image resolution plus a second positional is a hard error.** `busy draw eror "Build
  Failed!"` cannot be an image draw, because images take no message. Report that, and
  name the near-match.
- **Did-you-mean by edit distance** against the local template names, which rule 2 has
  already enumerated.

### 3.3 Inert flags are errors

`busy draw`'s `--help` is the union of Style, Placement, Scroll, Delivery, `--var` and
`--opacity`, grouped by `next_help_heading`. Some of those are meaningless for some
inputs.

**Passing a flag that cannot apply to the resolved input is a hard error, not a warning
and not a silent no-op.** `--font` on an image draw fails. `--opacity` on a text-only
template fails. `--var` on an asset draw fails. This keeps faith with §5's "never
silently do nothing", and it is the only defence against the union surface quietly
swallowing a typo.

The check runs after resolution, since resolution determines what applies.

### 3.4 `--id` under `draw`

The architecture doc's §5.3 gives `text` an `--id` defaulting to `message`, so repeat
invocations update in place rather than accumulating. Extending that:

| Resolved input | `--id` behaviour |
|---|---|
| text (`busy text`) | defaults to `message` |
| asset or stock image | defaults to `image` |
| template | **error** — element ids come from the template (§3.3) |
| `--file` | **error** — ids come from the payload |

`--keep` and replace-by-default (`DELETE display/draw` then POST) are unchanged from
§5.3 and apply to every form.

### 3.5 `template run` and `draw` are one code path

`busy template run <name>` remains the canonical, discoverable spelling and is the only
form `busy template --help` documents. `busy draw <name>` is the short verb, nine
characters shorter, and resolves to the same implementation.

`draw` is a distinct top-level verb rather than a level-skipping alias
(`busy template <name>`) on purpose. Skipping the verb would put user-created template
names in competition with `list | show | validate | run`, so a template directory named
`show` would silently list templates instead of drawing. That is the §1 rule violated
one level down, and worse than the original case because the colliding names are
user-created.

---

## 4. Templates

### 4.1 Templates live on the Mac

Templates stay in `~/.config/busy/templates/`. Their assets sync to the device.

This is forced by the API, not chosen: **the device has no concept of a template.**
There is no "run template X" endpoint; `display/draw` takes a fully-formed
`DisplayElements` body. Storing a `.toml` on the device would mean uploading it and then
`storage/read`-ing it back on every draw in order to substitute variables locally — an
extra round trip and flash wear, to make the device a file server for a file only the
client reads.

Sharing templates across machines is served by making `~/.config/busy/templates` a git
repository, plus a `--template-dir` flag, not by round-tripping through the bar.

### 4.2 Required variables come from the template, not the CLI

The message positional on `draw` and `template run` is **optional**. `busy draw
build-complete` is valid for a template whose text is a literal.

Whether a template needs a message is declared by whether it references one:

- Render with `Environment::set_undefined_behavior(UndefinedBehavior::Strict)`, so a
  template referencing `{{ message }}` with nothing supplied fails rather than rendering
  `text: ""` and hitting the device's `minLength: 1`.
- Use `Template::undeclared_variables(false)` — minijinja's static analysis — to catch
  it *before* rendering and produce a real message: *"template `error` requires variable
  `message`; pass it as the positional argument or `--var message=…`."*

The same call powers `busy template show <name>`, which lists the variables a template
accepts, and `busy template validate`.

**Optional variables need no CLI machinery.** No `[vars]` schema block in
`template.toml`. A template author who wants a default writes
`{{ message | default("Done") }}` and minijinja's own filter handles it.

One caveat: `undeclared_variables` does not follow includes, imports or inheritance.
Templates here are single self-contained files, so `busy template validate` must
**reject** `{% include %}`, `{% import %}` and `{% extends %}` rather than silently
under-report a template's requirements.

The positional binds to the `message` variable. Supplying both the positional and
`--var message=…` is an error.

### 4.3 `rect` and `countdown` are template-only for v1

`RectangleElement` (`width`, `height`, `radius`, `fill`, `fill_colors`) and
`CountdownElement` (`timestamp`, `direction`, `show_hours`) are first-class element
kinds and come along free via §7's "a template deserializes directly into
`DisplayElements`". They get **no dedicated subcommands** in v1.

They are worth knowing about because they eliminate the main reason to want dynamic
images. A progress bar on the 72×16 front display is two rectangles — a track and a fill
— where each tick upserts the fill's `id` with a new `width`: zero uploads, zero flash
writes, one request per frame. "Time remaining" is a native `countdown` that ticks
on-device with no requests at all.

Both are inherently multi-element and scripted, which is what templates are for. Revisit
if a dedicated verb turns out to be wanted.

---

## 5. Images and assets

### 5.1 Two-step by design

Drawing a local file takes two commands:

```sh
busy asset upload ./stop.png
busy draw stop.png
```

`busy draw` never uploads. Collapsing this into `busy draw ./chart.png` would require
probing the local filesystem to decide whether a name is a local file or an uploaded
asset — a shape rule, and the worst kind, because a local `stop.png` in the cwd would
silently shadow the uploaded `stop.png` the user meant, and both draw *an* image.

Local re-encoding for the target display (via the `image` crate) still happens on
`asset upload`, as §7 specifies.

### 5.2 What the Assets namespace offers, and what Storage adds

Per the vendored spec, `/busybar/assets/upload` supports exactly:

- `POST` — write one file (`application_name` and `file` as query params, body
  `application/octet-stream`). Overwrites in place.
- `DELETE` — delete **every** asset belonging to an app.

No list, no read, no per-file delete. `/busybar/storage/*` has `list`, `read`, `remove`
and `write`, but is confined to paths matching `^/ext(/[a-zA-Z0-9._\-]*)*$`, and the
spec never says where an app's assets live inside `/ext`.

**The device answers what the spec does not.** See §5.4 — app assets are at
`/ext/user_assets/<application_name>/`, so `storage/list` and `storage/read` do reach
them. Per-file delete still does not work.

### 5.3 Correction to the architecture doc

§7 says to check whether `storage/write` "converts server-side" and could replace the
local path. The spec gives no support for this: `storage/write` is a raw
`application/octet-stream` write to an `/ext/...` path with no documented conversion.
Treat the parenthetical as withdrawn. Local re-encoding stays the CLI's job.

### 5.4 Device probe — results

Run against a physical bar (API 25.0.0) on 2026-08-09. Reproducible with
`scripts/probe-device.sh`; re-run it after a firmware update and diff.

| Question | Answer |
|---|---|
| `/ext` layout | `apps_assets`, `user_assets`, `apps_data`, `update`, plus `Manifest` — **no `assets` dir**; the spec's `StorageList` example was generic |
| Where app assets live | `/ext/user_assets/<application_name>/` |
| Enumerate an app's assets | **Yes** — `storage/list` returns `[{type, name, size}]` |
| Read an asset's bytes back | **Yes** — `storage/read` returned all 73 bytes, byte-identical |
| Per-file delete | **No** — `storage/remove` on a real asset path returns `400 Bad Request` and the file survives |
| Delete all of an app's assets | Yes — `DELETE assets/upload?application_name=…`, which removes the directory itself |
| Draw referencing a missing asset | `400 {"error":"Failed to decode image /ext/user_assets/<app>/<file>."}` |
| Stock images | `/ext/apps_assets/shared/images/*.image`, enumerable (`checkmark_front_8x8.image`, …) |
| Stock sounds | `/ext/apps_assets/shared/sounds/*.snd` |

Two behaviours to code around:

- **`storage/remove`'s return value carries no signal.** It returned `OK` for a path
  that did not exist and `Bad Request` for one that did. Never trust it; confirm with a
  subsequent `list`.
- **An app with no assets 400s on `list`**, because delete-all removes the directory
  rather than emptying it. `busy asset list` must render that as "no assets", not as an
  error.

The missing-asset result is the important one: it is a **loud 400 that names the
resolved path**, not a silent 200. So there is no fourth silent-failure mode for the
architecture doc's §5, and no way for a stale presence check to produce a blank bar —
the worst case is a failed draw with a legible error.

### 5.5 Device-authoritative asset sync

Because presence is verifiable (§5.4), the CLI keeps **no local state**. There is no
`state.rs`, no `~/.local/state/busy/`, and no `--force-upload`.

Sync for a referenced asset:

1. `storage/list?path=/ext/user_assets/<app>` — one request per sync, not per asset.
2. Name absent → upload.
3. Name present, size differs → upload.
4. Name present, size matches → `storage/read` and compare content hashes; upload on
   mismatch.

Step 4 is affordable because these files are tiny — the probe asset was 73 bytes, and a
full-width image for the 72×16 front display is a few kilobytes. Size is the cheap
pre-filter; the hash makes it exact.

This is both simpler and more correct than a local cache: nothing to invalidate after a
factory reset, an upload from another machine, or an out-of-band `asset delete`.

**Commands:**

- `busy asset list` reports device truth from `storage/list`, printing "no assets" on
  the 400 described in §5.4.
- `busy asset delete` remains **all-or-nothing** for an app, because per-file delete
  does not work. Say so in its `--help`, and confirm before running it interactively.
- A future `busy asset stock` could enumerate `/ext/apps_assets/shared/images` to power
  `stock_path` completion. Phase 5 at the earliest.

**Graceful degradation.** `/ext/user_assets/<app>/` is undocumented — it was learned
from the text of a 400. If `storage/list` on `/ext/user_assets` ever fails, fall back to
unconditional upload, which is always correct because uploads are idempotent overwrites.
That keeps a firmware change from breaking the tool; it only makes it chattier.

---

## 6. Consequent changes to the architecture doc

| Section | Change |
|---|---|
| §3 | Replace all examples with §2 of this document. Drop design goal 1's `-m` framing; the fast common case is `busy text "…"`. |
| §4 | `cmd/image.rs` → merged into `cmd/draw.rs`. Add `cmd/draw.rs`. No `state.rs` — sync is device-authoritative (§5.5). |
| §5 | No fourth silent-failure mode: a draw referencing a missing asset returns a 400 naming the path (§5.4). Add the two `storage` quirks instead. |
| §5.3 | `--id` defaults per resolved input (§3.4); `--id` is an error for templates and `--file`. |
| §6.1 | Delete `args_conflicts_with_subcommands` and the flattened `TextArgs`. Subcommand always required. |
| §6.2 | Add `--opacity` to the shared surface. Add the post-resolution inert-flag check. |
| §7 | Withdraw the `storage/write` conversion claim. Add strict undefined behaviour, `undeclared_variables`, and the include/import/extends rejection. |
| §8 | Update the golden-payload test's command line. Add: resolution-order tests, inert-flag-is-an-error tests, a missing-required-variable test, and a stdin test. |
| §9 | Phase 0's probe is **done** (`scripts/probe-device.sh`, results in §5.4). Phase 1 is `busy text`. Phase 3 ships `draw` with stock and asset resolution only. Phase 4 adds template resolution to the existing `draw`. |

The phase note is worth expanding, because `draw` spans two phases: it ships in Phase 3
able to resolve stock paths and asset names, and Phase 4 inserts template lookup ahead
of the asset fallback. Rule 2 simply never matches until templates exist.

Updated golden-payload test command:

```sh
busy text -x 0 -y 8 --align mid_left --font small --color 0xFF0000FF "Goodbye, World!"
```

---

## 7. Decisions, in one place

| Decision | Rationale |
|---|---|
| No bare top-level positional | Subcommand shadowing; `busy "$MSG"` is silently wrong when `$MSG` is `clear` |
| No implicit default subcommand | Its only job was making `-m` parse without one |
| No `-m` / `--message` | Positional replaces it; nothing shipped, so no compatibility cost |
| `text` positional, `-` for stdin | CI messages usually arrive in a pipe |
| `text` requires a message | `Text` is `minLength: 1` |
| `draw` positional optional | A template may take no variables |
| `busy image` deleted, folded into `draw` | One verb for "display this"; the override surface was already shared |
| `draw` resolution: `shared/` → template dir → asset | Lookup rules, not shape rules; inspectable via `template list` and `--dry-run` |
| `--as` and `--dry-run` as escape hatches | Resolution is always forceable and always previewable |
| Image resolution + second positional = error | Turns a typo'd template name into a loud failure |
| Inert flags are hard errors | The union surface must not swallow typos |
| `draw` is a top-level verb, not `busy template <name>` | A level-skipping alias would put user-created names against `list\|show\|validate\|run` |
| `rect` / `countdown` template-only in v1 | Inherently multi-element and scripted |
| Templates on the Mac | The device has no template concept; storing them there adds a round trip |
| Strict undefined + `undeclared_variables` | The template declares its own requirements; no `[vars]` schema needed |
| `validate` rejects include/import/extends | Static analysis would otherwise under-report requirements |
| Images stay two-step | Auto-upload needs a filesystem shape rule that silently shadows uploaded assets |
| Device-authoritative sync, no local state | The probe showed `storage/list` and `storage/read` reach `/ext/user_assets/<app>/`; querying is simpler than a cache and has no staleness modes |
| `asset delete` stays all-or-nothing | Per-file `storage/remove` returns 400 on real asset paths and the file survives |
| Fall back to unconditional upload if `storage/list` fails | The asset path is undocumented; uploads are idempotent, so degradation costs only chattiness |

---

## 8. Rendering — measured on hardware

Run against a physical bar (API 25.0.0) on 2026-08-10 by drawing known payloads and
reading frames back through `GET /screen`. `GET /screen?display=0` returns the front
panel as base64 RGB888 (72×16×3 = 3456 bytes); `display=1` returns the back panel as
base64 4-bit greyscale (160×80÷2 = 6400 bytes), matching "16 greys".

### 8.1 The device's implicit `align` is `top_left`

Neither the OpenAPI document nor `busylib` supplies a default: `align` has no `default:`
key in the schema — pointedly, since its siblings `display` and `x`/`y` do — and
`busylib` models it as `Option<Align>` with `skip_serializing_if`, so an unset value is
simply absent from the JSON.

Drawing the same text at `(0,0)` with the key omitted produces a frame byte-identical to
`align: "top_left"`. **The device defaults to `top_left`.** This is now asserted by
`scripts/probe-device.sh`, so a firmware change to it will be caught rather than quietly
shifting every unspecified layout.

### 8.2 `align` decides whether the element is visible at all

`align` picks *which point of the element* sits at `(x, y)`, so it governs the direction
the element extends. At `(0,0)`, five of the nine values render **nothing** — a frame
byte-identical to a cleared screen — while the device still returns `200 OK`:

| `align` at (0,0) | Result |
|---|---|
| `top_left` (and omitted) | fully visible |
| `top_mid`, `mid_left`, `center` | partly visible |
| `top_right`, `mid_right`, `bottom_left`, `bottom_mid`, `bottom_right` | **blank** |

This is a distinct failure mode from an out-of-bounds anchor, and a bounds check that
only tests the anchor point is blind to it, because `(0,0)` is in bounds. The CLI warns
on it locally (plan Task 6).

### 8.3 Text is clipped silently when it overflows

Approximate inked width per character, measured by drawing known strings:

| Font | px/char | Chars fitting the 72px front panel |
|---|---|---|
| `tiny` | ~2.6 | ~27 |
| `small` | ~3.2 | ~22 |
| `large` (default) | ~5.4 | **~13** |

`Hello, World!` in `large` inks 70px and fits with 2px to spare; `Build Failed!` inks
68px. `Deployment completed OK` runs past the right edge, and the device clips it
silently with a `200 OK` — no error, no ellipsis, just a half-drawn glyph. Centred, an
overlong message loses both its head and its tail. The CLI estimates the width and warns,
pointing at `--width`/`--scroll-rate`.

### 8.4 CLI defaults: centre of the display

The CLI overrides the device's `top_left` with **`align = center`**, anchored at the
centre of whichever panel is selected — `(36, 8)` on the front, `(80, 40)` on the back.
The goal is that the zero-argument case, `busy text "hi"`, looks deliberate rather than
merely correct.

Verified on hardware at `(36, 8)`: `Hi` centres at x 30–40, `Build Failed!` at x 2–69,
and `Hello, World!` at x 1–70, with descenders (`gjpqy`) clearing the bottom edge. This
also sidesteps §8.2 entirely, since the default anchor is nowhere near an edge.

Two consequences, both accepted:

- An overlong message now clips at **both** ends rather than just the tail, which makes
  the §8.3 warning load-bearing rather than a nicety.
- `busy` now sends `align` explicitly where §2's original design omitted it. That
  omission was correct while the device's behaviour was unknown; it has been measured,
  and a deliberate default beats an undocumented one. The architecture doc's own example
  config already showed `align = "center"`, so the two now agree.

---

## 9. Follow-ups carried out of the foundation phase

Raised by the whole-branch review of the Phase 1–2 implementation and deliberately not
fixed there. Each is real; none blocks the foundation.

1. **A fourth invisibility mode is uncovered.** `validate::bounds_warnings` catches an
   out-of-bounds anchor, an anchor whose `align` pushes the element off-screen, and text
   wider than the panel. It does not catch short text positioned so near an edge that most
   of it falls off — `busy text -x 70 --align top_left "Hello"` leaves about 2px visible
   and warns nothing. The span arithmetic that would catch it was removed because it
   produced a factually false warning ("11px does not fit 72px") when it fired for
   positional reasons. Reinstating it needs the position and width checks kept separate.

2. **The replace-by-default flicker.** Measured and documented in the architecture doc
   §5.3. The cheap fix — skip the `DELETE` when the id set being written covers the one
   last written — needs a small state file keyed by `(addr, application_name)` and reset
   by `busy clear`. It is a heuristic: an out-of-band writer invalidates it, so it must
   degrade to clearing rather than to leaving stale elements.

3. **`FromArgName::accepted()` hand-duplicates clap's snake_case spellings.** Adding a
   variant to `FontArg`/`AlignArg`/`ScreenArg` without updating the matching list would
   silently produce a stale error hint with no compiler signal. Drive it from
   `T::value_variants()` instead.

4. **Precedence tests sample narrowly.** The flag→env→file→default chain now has
   end-to-end coverage for `addr`, but `app`, `token`, `http_timeout`, and `color` are
   only covered at some layers.

---

## 10. Open, deliberately

- **`busy status` output shape** — not designed here.
- **Config file layout** — unchanged from architecture doc §6.5.
- **Whether `DELETE`-then-`POST` visibly flickers** (§5.3 of the architecture doc) —
  still to be measured; the fallback is specified there.
