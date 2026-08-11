# Phase 4a — templates

**Design spec, 2026-08-11**

Phase 4 of `docs/busy-cli-architecture.md`, split in two. Phases 1–3 shipped: `busy text`,
`busy clear`, `busy asset upload|list|delete`, and `busy draw` for uploaded assets, device
stock paths, and raw `--file` payloads.

**4a is this document: the template engine, `busy template *`, and template resolution in
`draw`.** 4b is automatic device-side asset sync, deferred to its own spec (§10).

The architecture doc is the authority on *what the device does*; the command-surface spec
(`docs/specs/2026-08-09-busy-cli-ux-design.md`) on *what the user types*. This document is
the authority on *how Phase 4a is built*, and corrects both where they conflict (§9).

---

## 1. Why 4a and 4b split here

A template is a directory holding a `template.toml` plus, optionally, its own image files.
Those image files have to reach the device before the template can draw.

But a great many useful templates carry no images at all: `error` is red text, `ok` is a
`shared/` stock checkmark, a build-status template is text plus a `rect` progress bar. None
of those needs an upload. Sync is required only by the subset of templates that ship their
own PNG.

So 4a delivers the whole engine and every subcommand, working end to end for text and
stock-image templates, with `busy asset upload` covering the image case manually. 4b removes
that manual step. Each half ships working software, and 4a is about the size Phase 3 was —
which matters, because Phase 3 ran eight tasks and still needed seven fix rounds.

**Prerequisites.** Triage units 1 and 2 land before implementation starts:

- **#12** — `busy draw --help` advertises `--until` and then exits 2. Phase 4a rewrites that
  region of the CLI surface (§4.3); fixing it first avoids doing the work twice.
- **#9 + #10** — the error-kind contract. `impl From<String> for CliError` classifies every
  string error as a usage error. Phase 4a adds a dozen new error paths; writing them against
  the fixed contract is free, and writing them against the broken one adds a dozen more sites
  for a later PR to clean up.

---

## 2. The pipeline

```
busy draw error "Build failed" --var code=500
  │
  ├─ discover::resolve(name)            → template dir, or NotFound + did-you-mean
  ├─ render::analyse(source)            → required variables; rejects include/import/extends
  ├─ bind(positional, --var)            → error on a missing variable, or on both message forms
  ├─ render::render(source, vars)       → TOML text, every substitution auto-escaped
  ├─ toml::from_str::<DisplayElements>  → the payload
  ├─ validate::offline(&payload, dir)   → duplicate ids, missing local asset files
  ├─ overrides::apply(payload, args, Kind::Template)
  ├─ asset presence check               → one storage/list; error naming the upload command
  └─ validate::bounds_warnings + send   → the existing path, unchanged
```

Everything above the asset-presence check is offline and pure. `--dry-run` therefore
exercises the entire interesting part of the phase without a device, which is what makes this
phase testable.

**The key inherited move, unchanged:** a template deserializes *directly* into
`busylib::model::assets::DisplayElements`. The template file **is** the API payload. That is
what makes `animation`, `rectangle`, and `countdown` elements reachable without this project
modelling them, and it is why the pipeline ends by joining the path `--file` already uses.

---

## 3. Components

```
src/
├── template/
│   ├── mod.rs        # NEW — Template, load(), the pipeline above
│   ├── discover.rs   # NEW — root resolution, list(), suggest()
│   ├── render.rs     # NEW — the only module naming `minijinja`
│   └── validate.rs   # NEW — offline checks
├── overrides.rs      # NEW — Kind + the applicability table, shared with --file
├── cmd/
│   ├── template.rs   # NEW — init | list | show | validate | run
│   └── draw.rs       # gains resolution rule 2; loses its ad-hoc override rejections
└── cli.rs            # gains TemplateCmd, --var, --template-dir
```

`render.rs` is the only module permitted to write `use minijinja::…`, for the same reason
`device.rs` owns `busylib` and `image.rs` owns the `image` crate: one file to fix when an
upstream layout moves. This is the third application of a rule that has now held twice.

Splitting into four files rather than one `template.rs` is deliberate. The four jobs —
finding a template, rendering it, checking it, and orchestrating those — are independent and
separately testable, and a single file doing all four lands past 500 LOC. `config.rs` at 593
is already the repo's largest file and was flagged for it.

### 3.1 `discover.rs`

Template root, highest precedence first: `--template-dir`, then
`~/.config/busy/templates` via the existing `etcetera` strategy. A missing root is not an
error — it means "no templates", exactly as a missing config file means "no config".

`list()` returns the directory names containing a readable `template.toml`. A directory
without one is skipped silently; it is not a template.

`suggest(name, candidates)` powers did-you-mean, using a hand-rolled Levenshtein distance
(~25 lines) with a threshold of `min(2, name.len() / 3 + 1)`. A `strsim` dependency for one
call site is not worth the tree.

**Template names** are validated against `^[a-zA-Z0-9._-]+$` — the same charset `AssetName`
uses. A name containing `/` is rejected before any filesystem access, so a template name can
never escape the root.

### 3.2 `render.rs`

Three responsibilities, all minijinja-facing:

**Auto-escaping every substitution.** minijinja renders over the raw TOML text before it is
parsed, which is what lets a variable appear in any field. That also means an unescaped `"`
in a value corrupts the document — and `message` is the most user-controlled value in the
tool, routinely arriving from `git log -1 --format=%s`. So a custom escaper is installed as
the default auto-escape for the template: `"` → `\"`, `\` → `\\`, and control characters
(newline, tab, carriage return) to their TOML escapes.

Digits, letters, and punctuation pass through untouched, so a numeric field
(`x = {{ pos }}`) is unaffected — the escaper only ever changes characters that would break a
TOML basic string. A template author who genuinely wants a variable to expand into TOML
structure opts out per-expression with minijinja's `| safe`.

**`UndefinedBehavior::Strict`**, so a template referencing `{{ message }}` with nothing
supplied fails loudly rather than rendering `text = ""` and hitting the device's
`minLength: 1`.

**`analyse(source)`** wraps `Template::undeclared_variables(false)` to report a template's
required variables *before* rendering, which produces a real error message rather than a
render failure — and is the same call powering `template show` and `template validate`.

`undeclared_variables` does not follow includes, imports, or inheritance, so a template using
them would silently under-report its requirements. `analyse` therefore **rejects**
`{% include %}`, `{% import %}`, `{% from %}`, and `{% extends %}` outright. Templates here
are single self-contained files.

### 3.3 `validate.rs`

Offline checks, none touching the device:

- **Duplicate element ids.** Ids are the handle for `--keep` updates, so a duplicate makes a
  template's own elements overwrite each other.
- **A referenced local asset file that does not exist** next to `template.toml`.
- **Non-ASCII text**, reusing `sanitize::to_ascii` rather than writing a second copy. This is
  a **warning, not an error**, matching how `busy text` treats the same input: the text is
  transliterated and the user is told. A template is not more fragile than a message.

Bounds and overflow checks come free: once rendered, a template *is* a `DisplayElements`, so
the existing `validate::bounds_warnings` applies unchanged. `template validate` runs it and
reports its warnings alongside the hard errors above. This is the payoff for the inherited
decision to deserialize into the API type.

### 3.4 `overrides.rs`

```rust
pub enum Kind { Text, Image, Stock, Template, File }
pub fn apply(payload: DisplayElements, args: &DrawArgs, kind: Kind)
    -> Result<DisplayElements, CliError>;
```

One table, one place, replacing the four hand-written rejection chains Phase 3 accumulated in
`main.rs` and `cmd/draw.rs`. Closes issue #7 as a side effect.

---

## 4. The applicability table

**Decision: a template takes the same override rule as `--file`.** Payload-level flags
override; per-element flags are hard errors.

| Flag | text | image / stock | template | `--file` |
|---|---|---|---|---|
| `--priority`, `--led` | builds | builds | **overrides** | **overrides** |
| `--keep` | yes | yes | yes | yes |
| `-x` `-y` `--align` `--screen` `--timeout` | builds | builds | **error** | **error** |
| `--opacity` | n/a | builds | **error** | **error** |
| `--id` | default `message` | default `image` | **error** | **error** |
| `--var` | n/a | **error** | yes | **error** |

An override applies **only when the flag is given**; absent leaves the template's own value
untouched. Substituting a default would silently overwrite a value the template never asked
to have replaced.

### 4.1 Why templates follow the `--file` rule

The architecture doc §7 assumes CLI flags override template values, and the command-surface
spec §3 says template invocation "already accepted the full Style/Placement/Delivery surface
as overrides". Both predate the measurement that settled `--file`: a payload may hold several
elements, and a single `-x 4` has no principled element to apply to.

A template has exactly that shape. Applying a per-element flag to the first element, or to
all of them, are both defensible and neither is obviously right — which is the definition of a
flag that should be refused rather than guessed. The error names the flag and points at the
alternative: edit the template, or expose the value as a variable.

The cost is that `busy draw ok --color yellow` does not work on a one-element template where
it would be unambiguous. Accepted: a rule whose behaviour depends on how many elements a
template happens to contain is harder to learn than a flat one, and the template author has a
better tool for it anyway (`{{ color | default("#00ff00ff") }}`).

### 4.2 Why `--var` is an error on a non-template

`busy draw logo.png --var x=1` cannot do anything. Under the standing rule that a flag which
parses but does nothing is a defect, it is refused rather than ignored.

### 4.3 `draw` gains `--var` and nothing else

Command-surface spec §3.3 calls for `draw`'s help to be "the union of Style, Placement,
Scroll, Delivery, `--var` and `--opacity`". **Rejected.** `--font`, `--color`, `--width`, and
`--scroll-rate` are per-element, so by §4 they would be errors on every input `draw` accepts —
flags that exist only to be refused.

That is issue #12's defect exactly: a flag advertised in `--help` that always exits 2. The
right response to an inapplicable flag is not to offer it. `DrawArgs` therefore gains `--var`
and keeps everything else it has.

---

## 5. Command surface

```sh
busy template init [--force]        # write the error/ and ok/ examples
busy template list                  # names, descriptions, required variables
busy template show <name>           # description, elements, required variables
busy template validate [<name>]     # every template when the name is omitted
busy template run <name> [message] [--var k=v]...
busy draw <name> [message] [--var k=v]...
```

`template run` is the canonical, discoverable spelling; `busy draw <name>` is the short verb.
They resolve to one implementation (command-surface spec §3.5).

**Concretely, one implementation means `template run` flattens `DrawArgs`** rather than
declaring a parallel set of placement and delivery options. Two structs that must stay in
step would drift on the first flag added to either. The only difference is that `run`'s name
positional is always resolved as a template, so `--as` is not offered there.

### 5.1 Resolution, complete

1. `<name>` begins with `shared/` → `stock_path`
2. **a template directory `<name>` exists under the template root → template** ← new in 4a
3. otherwise → an app asset path → a single `ImageElement`

Rule 2 is the insertion Phase 3 shaped `cmd::draw::resolve` to accept. `--as
template|image|stock` forces the interpretation for pathological cases.

### 5.2 The message positional and the typo guard

The second positional is **optional** and binds to the `message` variable. Whether a template
needs it is declared by whether the template references it — there is no `[vars]` schema
block, and an optional variable is expressed with minijinja's own
`{{ message | default("Done") }}`.

Supplying both the positional and `--var message=…` is an error.

**A second positional on a non-template resolution is a hard error**, because images take no
message. This is the typo guard: `busy draw eror "Build Failed!"` cannot be an image draw, so
it reports the near-match from `suggest()` rather than failing later as a doomed asset draw.

### 5.3 `template init`

Creates the template root and writes `error` and `ok`. Refuses to overwrite an existing
template directory unless `--force`, and reports what it wrote.

The examples are the documentation — they are how a user learns the format — so they are
commented and deliberately cover both interesting cases: `error` is text plus a `shared/`
stock icon and a required `message` variable; `ok` is text with an optional one.

Neither shipped example carries a local PNG, so both work in 4a without asset sync.

---

## 6. The asset presence check

A template referencing a local file (`path = "stop.png"` beside `template.toml`) needs those
bytes on the device. 4a does not upload them; it checks and explains.

One `storage/list` call — the existing `Device::list_assets` — confirms whether the name is
present. Absent is a usage error:

```
template `error` references `stop.png`, which is not uploaded.
Run: busy asset upload ~/.config/busy/templates/error/stop.png
```

This is literally step 1 of the command-surface spec's §5.5 sync, so 4b adds steps 2–4 rather
than replacing anything. The cost is one round trip per template run that references a local
asset; templates using only text and `shared/` paths make no such call.

**Graceful degradation**, inherited from §5.5. `/ext/user_assets/<app>/` is undocumented — it
was learned from the text of a 400 — so the check must never be the reason a draw fails.
Precisely, given `Device::list_assets` already maps the directory-missing 400 to an empty
list:

- **`Ok(entries)`** — authoritative. A referenced name absent from `entries` is genuinely not
  on the device, including when `entries` is empty. Error, as above.
- **`Err(_)`** — the listing itself failed. **Skip the check entirely** and let the draw
  proceed to the device.

A firmware change must not break the tool; it may only make the resulting error later and
worse than the one this check would have produced.

---

## 7. Errors

Reuse `CliError`; the exit-code contract is unchanged (0 success, 1 runtime, 2 usage). Every
new case is a usage error, and every one fires before any device contact:

- a template name that does not resolve — with did-you-mean when one is close
- a required variable with no value supplied
- both the positional and `--var message=…`
- a `--var` argument not in `k=v` form
- `{% include %}`, `{% import %}`, `{% from %}`, or `{% extends %}` in a template
- a minijinja syntax or render error, quoting the template name and minijinja's own message
- rendered output that is not valid TOML, or not a valid `DisplayElements`
- duplicate element ids within a template
- a referenced local asset file missing from the template directory
- a referenced asset absent from the device (§6)
- any flag the §4 table marks an error for the resolved kind

---

## 8. Testing

The unit tests carry the weight, because the whole pipeline up to the presence check is pure.

- **`render.rs`:** the escaper against `"`, `\`, newline, tab, and a numeric field proving
  digits pass through unchanged; `| safe` opting out; `Strict` failing on an undefined
  variable; `analyse` reporting required variables; each of the four rejected constructs.
- **`discover.rs`:** root precedence; a directory without `template.toml` skipped; a name
  containing `/` rejected; `suggest` returning a near-match and staying silent on a distant
  one.
- **`template/validate.rs`:** duplicate ids; a missing local asset file; non-ASCII text.
- **`overrides.rs`:** every cell of the §4 table — this is the one place a table is the test.
- **`tests/template.rs`:** the five subcommands, `init --force`, and resolution rule 2
  including `--as` and the second-positional guard.
- **Golden snapshot** of a rendered multi-element template payload via `--dry-run`, pinning
  that a template really does produce the same wire bytes a hand-written payload would.
- **wiremock** for the presence check, including the degradation path.
- **One real-device run:** `busy template init`, then `busy draw error "Build failed"`, read
  back off the panel. A rendered template and a broken one both exit 0 — only the frame
  proves it.

---

## 9. Corrections to the inherited specs

Both are recorded here rather than silently diverged from; the implementation plan carries
them into the source documents.

1. **Architecture doc §7 and command-surface §3** assume CLI flags override template values
   generally. Corrected by §4: payload-level flags override, per-element flags are errors,
   matching the `--file` ruling.
2. **Command-surface §3.3** calls for `draw` to accept the union of every arg group.
   Corrected by §4.3: `draw` gains `--var` only, because the rest would be permanently
   inapplicable.

---

## 10. Out of scope for 4a

**Deferred to 4b:** content-hash asset sync, `storage/read` comparison, automatic upload of a
template's own images.

**Deferred beyond Phase 4:** dedicated `rect` and `countdown` verbs (they remain reachable
inside templates, which is the point of deserializing into the API type); template sharing
via git beyond `--template-dir`; `busy status`; asset stock enumeration for completion.

**Already recorded elsewhere:** the fourth invisibility mode and the replace-by-default
flicker (command-surface §9); the `--screen` dual-meaning draw-time warning (Phase 3 spec
§10, now issue #14, possibly moot since the device rejects oversized draws outright).

---

## 11. Risks

**minijinja is a real dependency with a real tree,** and the third whose surface is large
enough to want containing (after `busylib` and `image`). Mitigated the same way both of those
were: one module may name it, and the feature set is kept to what is used rather than the
default. Verify the actual feature requirements while implementing — `undeclared_variables`
and custom auto-escaping must both survive whatever is trimmed.

**Rendering before parsing remains structurally sharp.** Auto-escaping fixes the realistic
failure — a quote in a commit message — but a template author using `| safe` on
attacker-controlled input can still produce arbitrary TOML. Accepted: templates are
local files the user wrote, `| safe` is opt-in and documented as unsafe, and the alternative
(parse first, substitute into strings only) forfeits variables in numeric fields, which both
inherited specs assume.

**`undeclared_variables` is static analysis and can over-report.** A variable referenced only
inside a `{% if false %}` branch is still reported as required. Acceptable — it errs toward
asking for too much rather than rendering an empty string onto the panel — but it must be
documented in `template show`'s output so the list reads as "variables this template
mentions", not "variables you must supply".
