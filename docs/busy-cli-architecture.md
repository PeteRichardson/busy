# `busy` — an ergonomic BUSY Bar CLI

**Architecture / implementation spec, revision 3**

Written to be handed to a coding agent as the design brief for building this project
from scratch.

- Revision 2 followed a source review of the existing `busylib` crate; the plan no
  longer includes writing an HTTP client.
- Revision 3 folds in the actual OpenAPI document (API 25.0.0), which settled the
  draw semantics (§2, §5.3), turned the priority trap into a handled 409 (§5.1), and
  surfaced the ASCII-only text constraint (§2).

---

## 1. Prior art — read this first

A Rust client for the BUSY Bar **already exists and is good**. Do not write another
one.

`github.com/foresterre/busybar-rust` (Martijn Gribnau, author of `cargo-msrv`),
dual MIT/Apache-2.0, publishes four crates:

| Crate | Purpose |
|---|---|
| `busylib` | HTTP + WebSocket client. **This project depends on it.** |
| `busybar` | A CLI that mirrors the API 1:1. Not what we're building. |
| `busylib-proto` | Protobuf types for the WebSocket status stream |
| `busybar-render` | Re-encodes device screen frames into common image formats |

**Two gotchas that cost time if you don't know them:**

1. **There is no `display` module in `busylib`.** The api modules are grouped by
   *OpenAPI tag*, not by URL path. `display/draw` and `audio/play` live under
   `Assets`; `display/brightness` and `audio/volume` under `Settings`; `/screen`
   under `Streaming`. It absolutely does draw — `client.assets().draw(&elements)`.
2. **The API prefix differs by target.** It's `/api` on a physical device and
   `/busybar` on the published OpenAPI spec / cloud. `busylib` exposes this as
   `ApiPrefix`; get it wrong and every call 404s.

### What `busybar` (the existing CLI) does *not* do

Its draw command is `busybar assets draw --file payload.json` (or `-` for stdin).
There is no message flag, no defaults, no config file, no templates, no dry-run —
nothing between the user and hand-written JSON. **That gap is this project.**

### Dependency policy

`busylib` was first published 2026-08-02 and reached 0.0.11 within two days; the
module layout has already moved once (`display`/`audio` were folded into `assets`
in 0.0.5). It is excellent code at a very early version by a single author.
Therefore:

- **Pin an exact version**: `busylib = "=0.0.11"`. Upgrade deliberately, reading the
  changelog.
- **Put a thin adapter between the CLI and `busylib`** — a single `src/device.rs`
  that owns every `busylib` import and exposes only what the commands need. This is
  normally over-engineering; at this churn rate it means an upstream break is a
  one-file fix instead of a shotgun edit. Commands import `crate::device`, never
  `busylib` directly.
- If upstream stalls, the dual license permits vendoring. The expensive asset is
  the schema knowledge, not the HTTP plumbing.
- Gaps found along the way are worth contributing upstream rather than working
  around locally.

`busylib` requires Rust 1.86. Default features are `reqwest` and `ws`; **disable
`ws`** (`default-features = false, features = ["reqwest"]`) unless a streaming
feature is actually being built — it pulls in tungstenite, prost, and futures.

---

## 2. Verified schema facts

Cross-checked against the actual OpenAPI document (`openapi: 3.1.0`,
`info.version: 25.0.0`) and `busylib` 0.0.11 source. These are authoritative — do
not guess or re-derive them.

Fetch and vendor the spec:

```sh
curl -sL "https://api.busy.app/busybar/openapi.yaml?Name=1.1.1" -o spec/openapi.yaml
```

Note the query parameter is `?Name=`, not Swagger UI's stock `?urls.primaryName=`,
and that `1.1.1` is a docs-site label — the API version is `25.0.0`. The spec's
paths are all prefixed `/busybar/...`; a physical device serves the same API under
`/api/...` (see `ApiPrefix` in §1). Diff this file after any firmware update.

### Draw request

```rust
DisplayElements {
    application_name: AppName,
    priority: Option<Priority>,              // 1..=100
    led_notification_color: Option<Color>,   // blinks the status LED
    elements: Vec<DisplayElement>,
}

DisplayElement {
    id: ElementId,
    lifetime: Option<Lifetime>,   // flattened: `timeout` XOR `display_until`
    x: Option<i16>,
    y: Option<i16>,
    display: Option<Screen>,      // front | back
    align: Option<Align>,
    kind: ElementKind,            // flattened, tagged by `type`
}

ElementKind = text | image | animation | countdown | rectangle
```

Spec-level constraints the newtypes enforce, worth knowing when writing error
messages and validators:

| Field | Constraint |
|---|---|
| `application_name` | `^[a-zA-Z0-9._-]+$` — **no spaces** in `--app` |
| element `id` | same pattern; required |
| `priority` | 1–100, **device default 50** |
| `elements` | `minItems: 1` — an empty draw is not representable |
| `x`, `y` | −4096..4095, default `0` |
| `display` | default `front` |
| `align` | **no default** |
| `led_notification_color` | `^#[a-fA-F0-9]{8}$` |
| `stock_path` | `shared/[a-z0-9_.]+$` |

The element array is a `oneOf` with a `discriminator` on `type`, which is exactly
the shape `busylib`'s internally-tagged enum models.

### Text element

```rust
TextElement {
    text: Text,                          // printable ASCII only — bitmap fonts
    font: Font,                          // REQUIRED, not optional
    color: Option<Color>,
    width: Option<u16>,                  // width of the label
    scroll_rate: Option<u32>,            // PIXELS PER MINUTE
    scroll_start_delay: Option<u32>,     // milliseconds
    scroll_repeat_delay: Option<u32>,    // milliseconds
}
```

`text` is `^[\x20-\x7E]+$` with `minLength: 1` — **printable ASCII only**, because
the fonts are bitmap ASCII. `color` defaults to `#FFFFFFFF`.

This matters more than it sounds. A message pasted from a chat client, a commit
subject, or anything that passed through a shell with smart quotes will contain
U+2018/U+2019, an en- or em-dash, or an emoji, and the device will reject the whole
request. **The CLI must sanitize before sending** — transliterate common Unicode
punctuation to ASCII (curly quotes → `'`/`"`, dashes → `-`, ellipsis → `...`,
non-breaking space → space), drop or replace anything else, and warn once on stderr
when it changed something. Do not surface a raw regex-mismatch error; a build
notification that fails because of a smart quote is a terrible experience.

### Enums (all `snake_case` on the wire)

- `Font`: `tiny`, `small`, `normal`, `condensed`, `bold`, `large`, `extra_large`,
  `global`
- `Align`: `top_left`, `top_mid`, `top_right`, `mid_left`, **`center`**,
  `mid_right`, `bottom_left`, `bottom_mid`, `bottom_right`
  — note it is `center`, *not* `mid_center`. Align is an **anchor point used
  together with** `x`/`y`, not an alternative to them.
- `Screen`: `front`, `back`

### Color

Serialized as **`#RRGGBBAA`**, and `busylib::types::Color::parse` is strict: it
requires the leading `#` and exactly 8 hex digits. No `0x` form, no 3- or 6-digit
shorthand, no names.

**The CLI must do its own lenient parsing** and construct
`Color::rgba(r, g, b, a)`. Accept `0xRRGGBBAA`, `#RRGGBBAA`, `#RRGGBB`, `#RGB`,
bare hex, and a small named table (`red`, `green`, `blue`, `white`, `black`,
`yellow`, `orange`, `cyan`, `magenta`). `busylib` provides `Color::RED`, `WHITE`,
`BLACK`, `GREEN`, `BLUE`, `TRANSPARENT` consts.

### Other endpoints in use

```rust
client.assets().upload(app, file, bytes)   // POST assets/upload, octet-stream body
client.assets().draw(&elements)            // POST display/draw
client.assets().clear(Some(app))           // DELETE display/draw  — a real endpoint
client.assets().delete(app)                // DELETE assets/upload — drops app assets
client.assets().play(&PlayAudio)           // POST audio/play
```

Image and audio sources are an untagged enum: `path` for a file in the app's own
asset directory, `stock_path` for a device built-in. Images also take
`opacity` (0–100).

### Validated newtypes

`busylib` uses validated newtypes throughout — `AppName`, `ElementId`, `Text`,
`AssetPath`, `StockPath`, `Color`, `Opacity`, `Priority`, `Volume`, `Brightness` —
constructed via a `TryIntoValue` trait returning `InvalidValue`. Let these do the
validation; **do not re-implement range checks in the CLI**, just surface the errors
with good messages.

### Draw semantics: upsert, not replace

**`POST display/draw` upserts elements by `id` into the app's existing set. It never
removes anything.** Elements persist until their `timeout`/`display_until` expires
or `DELETE display/draw` removes them.

The spec doesn't state this in prose; it follows from three things:

1. `AnimationElement.await_previous_end` is documented as *"If the element has been
   created before and this flag is true, the previous range will finish before the
   requested one starts."* Elements survive across requests and are matched by `id`.
2. `elements` has `minItems: 1`, so POST cannot express an empty set — which is why
   `DELETE display/draw` exists as a separate operation. Replacement semantics with
   no representable empty state would make clearing impossible.
3. `timeout`/`display_until` are per-element, so elements expire independently of
   the request that created them.

See §5.3 for the CLI consequence, which is significant.

---

## 3. Target UX

```sh
busy -m "Hello, World!"
busy -x 0 -y 8 --align mid_left --font small --color 0xFF0000FF -m "Goodbye, World!"
busy --template error -m "Build Failed!"
busy --timeout 30 -m "deploy done"
busy asset upload ./stop.png
busy clear
```

Design goals in priority order:

1. **Fast and boring for the common case.** `busy -m "..."` is the thing typed a
   thousand times.
2. **Extensible by addition.** A new area of the API is a new subcommand module and
   nothing else.
3. **Templates as a first-class concept** — several elements, variables, assets.
4. **Never silently do nothing.** See §5 on priority; this device has two distinct
   ways to accept a request and render nothing.

Non-goals for v1: WebSocket streaming, frame capture/mirroring (`busybar` already
does these well — point users there), Matter, firmware update, Wi-Fi provisioning.

---

## 4. Layout

Single crate. `busylib` is the reusable library; there's no second one to write.

```
busy/
├── Cargo.toml
├── spec/openapi.yaml          # vendored OpenAPI 3.1 document, API v25.0.0
└── src/
    ├── main.rs
    ├── cli.rs                 # clap definitions
    ├── device.rs              # THE ONLY module that imports busylib
    ├── config.rs              # file/env/flag resolution + Defaults
    ├── ctx.rs                 # resolved runtime context
    ├── color.rs               # lenient color parsing -> busylib Color
    ├── output.rs              # human / --json / --dry-run
    ├── template.rs
    └── cmd/
        ├── mod.rs
        ├── text.rs
        ├── image.rs
        ├── clear.rs
        ├── asset.rs           # upload | list | delete
        ├── template.rs        # list | show | validate | run
        └── status.rs
```

Package `busy-cli`, `[[bin]] name = "busy"`. The crate names `busybar` and
`busylib` are taken; the binary name `busybar` is taken; `busy` is free.

If the template layer later proves reusable by other tools, split it into a
`busy-scene` crate **then** — not preemptively.

---

## 5. The three ways a correct-looking draw shows nothing

Each of these produces a request the device accepts (or rejects with a status code
nobody reads) and a bar that doesn't show what the user meant. Handle all three
explicitly.

### 5.1 Priority

A draw request is accepted only when its priority is **greater than or equal to**
that of the currently running system app. Equal-priority requests from a *different*
`application_name` override what's on screen. System levels:

| System state | Priority |
|---|---|
| stub / poweroff app | 0 (always preemptable) |
| any standard built-in app | 10 |
| **active BUSY / CUSTOM work session** | **90** |

The API accepts 1–100; 0 is reserved. **The device default is 50** — below a work
session, so an unset priority loses exactly when the user is at their desk.

**This fails loudly, which is good news:** `POST display/draw` returns **409**,
*"Requested priority level is below that of currently active app."* Decisions:

- Default priority to **95**. This CLI is for deliberate, user-invoked
  notifications; they should beat a running session. Overridable in
  `[defaults]` and via `--priority`.
- Named aliases: `--priority low|normal|high|urgent` → 10 / 50 / 95 / 100.
- **Map 409 to a real message**, not a bare status: *"The bar is running a focus
  session at priority 90. Retry with `--priority 95`, or set `priority` in
  ~/.config/busy/config.toml."* This is the highest-value error string in the whole
  tool — write it before anything else in the error module.
- Document the priority table in `busy --help` for the flag.

### 5.2 Lifetime

`Lifetime` is `timeout` (seconds, 0 = no timeout) **xor** `display_until` (Unix
seconds). Mutually exclusive — enforce with a clap arg group.

- `--timeout <secs>` and `--until <rfc3339|unix>`, conflicting.
- Default: **no timeout** (the message persists until cleared or overridden), since
  that matches "force a message to my bar." But a CI-oriented template should be
  able to set one, and `--timeout 30` should be an easy habit.

### 5.3 Stale elements — replace vs. compose

Because draw upserts by `id` and never removes (§2), this sequence leaves the wrong
thing on screen:

```sh
busy --template error -m "Build Failed!"   # draws ids `icon` + `message`
busy -m "hi"                               # draws id `message` only
#  → the red stop sign is STILL THERE, next to "hi"
```

Users will hit this on day one. The CLI therefore needs explicit screen semantics:

- **Default: replace.** `DELETE display/draw?application_name=<app>` followed by the
  POST. This matches the stated goal — force *this* message to the bar — and makes
  every invocation independent of history.
- **`--keep` (or `--compose`): upsert only.** Skip the DELETE, for incrementally
  updating one element of a multi-element layout. This is the mode a status-widget
  script wants.
- **`--id <name>`**, defaulting to something stable like `message`, so repeated
  `busy -m` calls update in place rather than accumulating.

Measure whether DELETE-then-POST visibly flickers. If it does, fall back to
replace-by-convention: track the id set the CLI last wrote (in a small state file
under `~/.local/state/busy/`) and overwrite or blank exactly those ids instead of
clearing. Don't build that unless the flicker is real.

> **Measured 2026-08-10 (API 25.0.0): the flicker is real.** Sampling `GET /screen`
> across a normal `busy text` replace draw shows a clean three-phase transition — old
> text, then the **cleared-screen frame**, then the new text (2 blank samples out of 30).
> The panel visibly blanks on every invocation, which is the common case, not an edge
> case.
>
> A refinement the original note did not anticipate makes the fallback cheaper than
> described. Because `POST display/draw` upserts by `id`, the DELETE is only needed when
> a *previous* draw left elements this one will not overwrite. If the id set about to be
> written is a superset of the id set last written, the POST alone replaces everything
> and no clear is required — which is exactly the repeated `busy text` case, where both
> sets are just `{message}`. The state file therefore only has to answer "which ids did I
> write last time?", and the DELETE becomes rare rather than universal.
>
> Deferred: this is a new subsystem (`state.rs`, removed in the plan's Task 4 when asset
> sync went device-authoritative) and is out of scope for the foundation plan. `--keep`
> already gives an escape hatch today for scripts that cannot tolerate the blank.

### 5.4 Bounds

The front display is 72×16 RGB; the back is 160×80 in 16 greys. Elements placed
outside those bounds render nothing, with no error. Add a local
`validate(&DisplayElements)` in the CLI that warns (not errors) on out-of-bounds
coordinates before sending. This is cheap and saves a lot of confused staring.

---

## 6. CLI design

### 6.1 Subcommands with an implicit default

`busy -m "Hello"` must work *and* subcommands must exist from day one. clap's
declarative idiom for this is `args_conflicts_with_subcommands`:

```rust
#[derive(Parser)]
#[command(name = "busy", version, args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(flatten)]    global: GlobalArgs,
    #[command(subcommand)] command: Option<Command>,
    #[command(flatten)]    text: TextArgs,   // the implicit `busy text ...`
}

#[derive(Subcommand)]
enum Command {
    Text(TextArgs),
    Image(ImageArgs),
    Clear(ClearArgs),
    Asset(AssetCmd),
    Template(TemplateCmd),
    Status(StatusArgs),
}
```

`busy -m "hi"` parses the flattened `TextArgs`; `busy text -m "hi"` takes the
subcommand path; both appear in `--help`. Every field in `TextArgs` must be
optional — error at runtime if neither a subcommand nor a message is present.

Do **not** implement this by rewriting `argv` to insert a default subcommand; the
declarative version keeps `--help` honest.

### 6.2 Option groups

These are the reusable unit — `text` and `template run` flatten the same structs, so
the option surface stays consistent for free. `next_help_heading` groups them in
`--help`, making the structure visible to users.

```rust
#[derive(Args, Clone, Default)]
#[command(next_help_heading = "Style")]
struct StyleArgs {
    #[arg(long)] font: Option<FontArg>,
    #[arg(long)] color: Option<ColorArg>,     // lenient parser, see §2
}

#[derive(Args, Clone, Default)]
#[command(next_help_heading = "Placement")]
struct PlacementArgs {
    #[arg(short = 'x', long)] x: Option<i16>,
    #[arg(short = 'y', long)] y: Option<i16>,
    #[arg(long)] align: Option<AlignArg>,
    #[arg(long)] screen: Option<ScreenArg>,   // front | back
}

#[derive(Args, Clone, Default)]
#[command(next_help_heading = "Scrolling")]
struct ScrollArgs {
    #[arg(long)] width: Option<u16>,
    /// Scroll rate in pixels per minute
    #[arg(long)] scroll_rate: Option<u32>,
    /// Milliseconds before scrolling starts
    #[arg(long)] scroll_start_delay: Option<u32>,
    /// Milliseconds between scroll cycles
    #[arg(long)] scroll_repeat_delay: Option<u32>,
}

#[derive(Args, Clone, Default)]
#[command(next_help_heading = "Delivery")]
struct DeliveryArgs {
    #[arg(long)] priority: Option<PriorityArg>,       // number or low|normal|high|urgent
    #[arg(long)] timeout: Option<u32>,
    #[arg(long)] led: Option<ColorArg>,               // led_notification_color
    /// Element id to write, so repeat invocations update in place
    #[arg(long, default_value = "message")] id: String,
    /// Compose onto what's already on screen instead of replacing it (see §5.3)
    #[arg(long)] keep: bool,
}
```

**`until` is not part of `DeliveryArgs`.** `DeliveryArgs` is flattened into both
`TextArgs` and `DrawArgs`, and `draw` has no use for `--until` — it addresses
several element kinds, not just text with a lifetime. Issue #12 was exactly this:
`--until` sat in the shared struct, so `draw --help` advertised it and then
rejected it at runtime. The fix (a prerequisite to Phase 4a) declares `until`
directly on `TextArgs`, pinned to the same "Delivery" help heading so the split
is invisible to `text --help`; `draw` simply never offers the flag.

Put the units in the doc comments. `scroll_rate` being pixels *per minute* is
surprising enough that it belongs in `--help`.

### 6.3 Global args

```
--addr <url>          device base URL      (env BUSY_ADDR, default http://10.0.4.20)
--api-prefix <p>      device | spec        (default device = /api)
--token <pin>         access key           (env BUSY_TOKEN, hide_env_values)
--app <name>          application_name     (default "busy")
--timeout-ms <ms>     HTTP timeout
--json                machine-readable output
--dry-run             print the payload, send nothing
-q / -v               verbosity
```

Note the two distinct timeouts: `--timeout-ms` is the HTTP request timeout,
`--timeout` is how long the element stays on screen. Name them so nobody confuses
them; consider `--http-timeout` if it still reads badly in `--help`.

### 6.4 Dispatch

Plain `match cli.command { ... }`, each arm calling `cmd::foo::run(args, &ctx)`.
No `Run` trait — it buys nothing without dynamic dispatch or plugins.

### 6.5 Configuration precedence

**Every CLI option is `Option<T>`. Do not use clap's `default_value`.**

Highest wins:

1. CLI flags
2. Environment (`BUSY_ADDR`, `BUSY_TOKEN`, …)
3. Config file `~/.config/busy/config.toml` (locate with `directories` or `etcetera`)
4. Template-supplied values
5. Built-in `Defaults`

This matters because templates supply values that CLI flags must override, which is
only expressible if "unset" is distinguishable from "explicitly set to the value
that happens to be the default." Defaults live in exactly one `Defaults` struct in
`config.rs`, never scattered across clap attributes.

```toml
addr = "http://10.0.4.20"
app  = "busy"

[defaults]
font     = "large"
align    = "center"
color    = "#ffffffff"
priority = 95
```

Note `font` is **required** by `busylib::TextElement`, so the resolver must always
produce one. `large` is the specified default.

Keep the access key out of `argv` — prefer env or config file so it stays out of
shell history and `ps`. Warn (don't fail) if the config file is world-readable.

### 6.6 `--dry-run`

Build the `DisplayElements`, `serde_json::to_string_pretty` it, print, and return
without calling the device. Because it's the same type `busylib` serializes, the
output is exactly the wire payload. Implement this in phase 1 — it's what the
snapshot tests assert against and the first thing wanted when the bar shows nothing.

---

## 7. Templates

**The key move: a template's `elements` deserialize directly into
`busylib::model::assets::DisplayElement`.** The template file is the API payload
minus its envelope, which means animation, countdown, and rectangle elements
come along for free without this project modeling them.

A template does *not* deserialize into `DisplayElements` itself: that type
requires `application_name`, which comes from `--app`/`BUSY_APP`/the config
file and must not be baked into a template. A thin `TemplateFile` wrapper
supplies the envelope and adds the `description` field this document's own
example uses. See `docs/specs/2026-08-11-phase-4a-templates-design.md` §2.1.

Pipeline:

```
load template → substitute vars → apply CLI overrides → sync assets → validate → draw
```

Self-contained directories so a template carries its own assets:

```
~/.config/busy/templates/
└── error/
    ├── template.toml
    └── stop.png
```

```toml
# error/template.toml
description = "Red stop sign plus an error message"
priority = 95

[[elements]]
id = "icon"
type = "image"
path = "stop.png"          # relative to the template directory
x = 0
y = 0
align = "mid_left"

[[elements]]
id = "message"
type = "text"
text = "{{ message }}"
x = 18
align = "mid_left"
font = "small"
color = "#ff0000ff"
scroll_rate = 600
```

- **Substitution:** `minijinja` over the raw TOML text *before* parsing — simplest,
  and lets variables appear in any field. Values from `-m` (binds to `message`),
  repeated `--var k=v`, and env. Every substitution is auto-escaped for a TOML
  basic string, so a quote in a commit subject cannot corrupt the document; `| safe`
  opts out.
- **Asset sync:** content-hash each referenced local file and skip upload if already
  present. `assets/upload` sends bytes as-is with no conversion, so the CLI must
  resize/re-encode images for the target display (`image` crate). This is the one
  part of the project with real work in it. Check whether `storage/write` (which
  converts server-side) can replace the local path — if it can, prefer it.
- **Send one request** with all of a template's elements. Upsert semantics (§2) mean
  several requests would also work, but one request means one priority check, one
  409 to handle, and no half-drawn intermediate state.
- **Element ids must be unique within a template**, and templates should use
  descriptive ones (`icon`, `message`) rather than `0`/`1`, since they're the handle
  for later `--keep` updates. `busy template validate` should reject duplicates.
- **Subcommands:** `busy template list | show <name> | validate <name> | run <name>`.
  `validate` should catch out-of-bounds coordinates, non-ASCII text, missing asset
  files, and bad enum values without touching the device.

---

## 8. Testing

- **Golden-payload test, highest value by far.** Assert that

  ```sh
  busy -x 0 -y 8 --align mid_left --font small --color 0xFF0000FF -m "Goodbye, World!"
  ```

  serializes to exactly the expected JSON. Flag/env/config/template precedence bugs
  surface here and almost nowhere else. Use `insta` over `--dry-run` output.
- **Precedence test:** CLI flags override template values; template values override
  built-in defaults.
- **Color parser table test:** every accepted input form → expected `#RRGGBBAA`.
- **ASCII sanitization table test:** curly quotes, em-dash, ellipsis, non-breaking
  space, an emoji → the expected ASCII, with the warning fired. Include a case where
  the whole message sanitizes to empty (`minLength: 1` would reject it) and assert a
  clear error.
- **409 handling test:** a `wiremock` server returning 409 must produce the priority
  guidance message from §5.1, not a raw status.
- **Stale-element test:** template run followed by a plain `busy -m` issues the
  DELETE by default, and does not issue it under `--keep`.
- **CLI surface:** `assert_cmd` + `trycmd` for help text and error cases.
- Transport behaviour is `busylib`'s problem and is already covered by wiremock
  tests upstream. Don't duplicate it. A single smoke test against a `wiremock`
  server through `device.rs` is enough to catch adapter mistakes.

---

## 9. Build order

Each phase ends in a working, committed, releasable state.

**Phase 0 — vendor the spec.**
`curl` the OpenAPI document into `spec/openapi.yaml` (§2) and skim `display/draw`,
`DisplayElements`, and `TextElement`. Everything downstream references it.

**Phase 1 — hello world.**
Crate skeleton, `device.rs` adapter, `busylib` client construction (URL, prefix,
token, timeout), `busy -m` end to end, `--dry-run`, `--json`.
*Done when:* `busy -m "Hello, World!"` lights up a real bar and `--dry-run` prints
the payload.

**Phase 2 — the option surface and the three failure modes.**
The four arg groups, `GlobalArgs`, config file + env resolution, the `Defaults`
struct, lenient color parsing, ASCII sanitization with warning, priority defaults
and aliases, **409 → guidance message**, lifetime flags, `--id`/`--keep` with
replace-by-default, bounds warnings.
*Done when:* the second example in §3 renders correctly, the golden-payload test
passes, a message appears during an active focus session, `busy -m "don't — really"`
renders without error, and a template run followed by `busy -m "hi"` leaves no stale
icon on screen.

**Phase 3 — assets and images.**
`busy asset upload|list|delete`, image conversion, `busy image`, content-hash
skip-if-present, `busy clear`.
*Done when:* a PNG can be uploaded and drawn in two commands.

**Phase 4 — templates.**
TOML dialect over `DisplayElements`, discovery, minijinja substitution, override
precedence, asset sync, `busy template *`. Ship `error` and `ok` templates as
examples.
*Done when:* `busy --template error -m "Build Failed!"` works.

**Phase 5 — polish and breadth.**
`busy status`, `clap_complete` shell completions, a `--version` that also reports
the device's API version. Then further API areas only as actually needed — for
frame capture and screen mirroring, point users at the upstream `busybar` CLI
rather than reimplementing.

---

## 10. Reference

- Existing Rust client and CLI: https://github.com/foresterre/busybar-rust
- `busylib` docs: https://docs.rs/busylib
- API docs (Swagger UI): https://api.busy.app/busybar/docs
- HTTP API overview: https://docs.busy.app/bar/dev/http-api
- Python client, best prose reference for device behaviour:
  https://github.com/busy-app/busylib-py
- Author's own layering style for comparison (Swift, UniFi Protect):
  https://github.com/PeteRichardson/Protect
