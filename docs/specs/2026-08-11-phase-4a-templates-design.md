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
  ├─ bind(positional, --var)            → error on a missing variable/both message forms,
  │                                        then sanitize_values: to_ascii each bound value,
  │                                        warning once if any changed (§3.3) — before render
  ├─ render::render(source, vars)       → TOML text, every substitution auto-escaped
  ├─ toml::from_str::<TemplateFile>     → description + elements (§2.1)
  ├─ TemplateFile::into_payload(app)    → DisplayElements
  ├─ validate::offline(&payload, dir)   → duplicate ids, missing local asset files
  ├─ overrides::apply(payload, args, Kind::Template)
  ├─ asset presence check               → one storage/list; error naming the upload command
  └─ validate::bounds_warnings + send   → the existing path, unchanged
```

Everything above the asset-presence check is offline and pure. `--dry-run` therefore
exercises the entire interesting part of the phase without a device, which is what makes this
phase testable.

### 2.1 A template is `DisplayElements` minus the app name, plus a description

The architecture doc says a template deserializes *directly* into
`busylib::model::assets::DisplayElements`. **Measured: it cannot, and the gap is one field.**

`DisplayElements::application_name` is required and has no default, so the architecture doc's
own example `template.toml` fails to parse with `missing field 'application_name'` — verified
against busylib 0.0.11. That field must not appear in a template anyway: which app owns the
draw comes from `--app`, `BUSY_APP`, or the config file, and a template hard-coding it would
defeat the whole precedence chain.

The doc's example also opens with `description = "..."`, which `DisplayElements` has no field
for. It has no `deny_unknown_fields`, so that line parses and is *silently discarded* —
leaving `template list` and `template show` with nothing to print.

So a template parses into a thin wrapper:

```rust
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateFile {
    pub description: Option<String>,
    pub priority: Option<Priority>,
    pub led_notification_color: Option<Color>,
    pub elements: Vec<DisplayElement>,
}
```

`into_payload(app)` then builds the real `DisplayElements`. `deny_unknown_fields` is the point
of doing it by hand: a typo'd key in a template becomes an error naming the key, rather than a
field silently ignored.

**The part of the inherited claim that matters survives intact.** `elements` is
`Vec<DisplayElement>` — busylib's own type — so `animation`, `rectangle`, and `countdown`
elements still come along free without this project modelling them. Verified end to end: a
template declaring a `rectangle` with `width`/`height` parses and serializes to exactly the
expected wire JSON, with no code here naming a rectangle. That is the whole reason for the
design, and only the envelope changed.

---

## 3. Components

```
templates/            # NEW — the shipped examples, embedded at compile time (§3.5)
├── error/
│   └── template.toml
└── ok/
    └── template.toml
src/
├── template/
│   ├── mod.rs        # NEW — TemplateFile, Template, load(), the pipeline above
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

**Correction made during implementation.** This section originally listed non-ASCII text as a
third offline check here, reusing `sanitize::to_ascii` to turn it into a warning rather than an
error. **That is unimplementable as written:** busylib's `Text` is a `string_newtype!` validated
by `printable_ascii`, and validation runs at *deserialization* — a non-ASCII character fails
inside `toml::from_str::<TemplateFile>` (i.e. inside `Template::render`, §2), before any
`validate.rs` check ever sees the value. The branch was provably unreachable and was removed in
Task 4; see the comment on `offline` in `src/template/validate.rs` for the fixture that proves
it (`toml::from_str` panics with `"invalid display text..."` before reaching `offline`).

What actually shipped, in Task 5, is better and belongs here instead:

- **Variable values** — the `--var` and positional-message inputs bound before rendering — are
  sanitized with `sanitize::to_ascii` *before* substitution (§2's bind step), warning once per
  invocation exactly the way `busy text` warns for its message. This is the case that matters
  in practice: the README's own example pipes `git log -1 --format=%s` into a template, and
  commit subjects contain smart quotes routinely.
- **Literal non-ASCII written directly into a template file** still hard-fails, with an error
  naming the template (`template \`x\` did not produce a valid template file: ...`). That is
  deliberate and different in kind from a bad variable: the template author owns that file and
  can fix it, and `busy template validate` is the tool that surfaces it.

The spec's stated goal — *a template is not more fragile than a message* — is unchanged, and is
now actually true for the case that matters: a stray smart quote piped in from the shell gets
the same sanitize-and-warn treatment `busy text` gives it, whether it arrives as a message or
lands in a template through `{{ message }}`.

Bounds and overflow checks come free: once rendered, a template *is* a `DisplayElements`, so
the existing `validate::bounds_warnings` applies unchanged. `template validate` runs it and
reports its warnings alongside the hard errors above. This is the payoff for the inherited
decision to deserialize into the API type.

### 3.4 The shipped examples are a directory, not a code table

**Adding an example template must be a commit, not a code change.** `templates/` at the repo
root holds them in exactly the installed layout — `templates/<name>/template.toml`, plus any
files the template carries — and the whole tree is embedded at compile time:

```rust
static EXAMPLES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/templates");
```

`include_dir` 0.7 pulls in one macro crate and nothing else, and needs no default features.
The alternative — an `include_str!` per file — requires editing Rust to add a template, which
is precisely what this is avoiding.

`init` walks `EXAMPLES.dirs()`, skipping any directory without a `template.toml` (the same
rule `discover.rs` applies to the installed root), and writes every file in each. Embedding
whole directories rather than single files means a template that ships a PNG works without
change when 4b lands.

**The guard that makes this safe is a test, not review.** `tests/examples.rs` iterates every
embedded template and runs the full offline pipeline on it: render with each required variable
bound to a placeholder, parse, and validate. Committing a template with a TOML typo, a
duplicate element id, or a reference to a file that isn't there fails the suite. Without that
test, "just commit a new one" means shipping unvalidated content to every user who runs
`template init`.

### 3.5 `overrides.rs`

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

**Concretely, one implementation means `DrawArgs` and `TemplateRunArgs` compose a shared
`DrawCommon`** (`opacity`, `--var`, `message`, placement, delivery) rather than each
declaring a parallel set of placement and delivery options. Two structs that must stay in
step would drift on the first flag added to either — this is what keeps them from drifting,
without offering `run` a `--file` or `--as` it does not need (§4 explains why neither applies
to a template). `name` is declared
separately on each of `DrawArgs` and `TemplateRunArgs` rather than folded into `DrawCommon`
too: it is the one field that means something different in each command (an asset name or a
`shared/…` built-in on `draw`, a template name and nothing else on `run`), so sharing its
declaration meant sharing its help text as well — `template run --help` advertised "Asset
name, or a `shared/…` device built-in", which described `draw`'s behaviour on a command that
rejects a `shared/…` name as an unusable template name. A duplicated field with genuinely
divergent semantics is not the drift risk `DrawCommon` exists to prevent; a help string that
contradicts the command's own behaviour is worse. The only behavioural difference between the
two commands is that `run`'s name positional is always resolved as a template, so `--as` is
not offered there.

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

Creates the template root and writes every example embedded from `templates/` (§3.4). An
existing template directory is **skipped, not overwritten**, unless `--force`. Reports each
name as written or skipped, so the outcome is never a silent no-op.

**Skip-by-default gives the maintenance workflow for free.** Commit a new template to
`templates/`, and a user who re-runs `busy template init` after upgrading gets exactly the new
one — their edits to `error` are left alone, because `error` already exists. No name filter,
no `--only`, no merge logic. `--force` exists for the other case: restoring a shipped example
someone has broken.

The examples are the documentation — they are how a user learns the format — so they are
commented, and the initial two cover both interesting cases: `error` is text plus a `shared/`
stock icon with a **required** `message` variable; `ok` is text with an **optional** one via
`{{ message | default("Done") }}`.

Neither carries a local PNG, so both work in 4a without asset sync. That is a property of
these two, not a rule — §3.4's embedding handles a template with assets, and such a template
would simply hit §6's presence check until 4b lands.

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
- **`template/validate.rs`:** duplicate ids; a missing local asset file. (Not non-ASCII text:
  see §3.3's correction — that check lives in `template/mod.rs::sanitize_values`, tested there
  and in `tests/template.rs` via a smart-quote `--var`/message value that gets transliterated
  with a warning, plus a literal smart quote in a template file that still hard-fails.)
- **`overrides.rs`:** every cell of the §4 table — this is the one place a table is the test.
- **`tests/template.rs`:** the five subcommands, `init` skipping an existing directory and
  `--force` overwriting it, and resolution rule 2 including `--as` and the
  second-positional guard.
- **`tests/examples.rs`:** every template embedded from `templates/`, run through the full
  offline pipeline — render with each required variable bound to a placeholder, parse,
  validate. This is the guard that makes "adding an example is just a commit" safe, and it
  must iterate the embedded set rather than naming `error` and `ok`, or it stops covering
  the next template added.
- **Golden snapshot** of a rendered multi-element template payload via `--dry-run`, pinning
  that a template really does produce the same wire bytes a hand-written payload would.
- **wiremock** for the presence check, including the degradation path.
- **One real-device run:** `busy template init`, then `busy draw error "Build failed"`, read
  back off the panel. A rendered template and a broken one both exit 0 — only the frame
  proves it.

---

## 9. Corrections to the inherited specs

All four are recorded here rather than silently diverged from; the implementation plan carries
them into the source documents.

1. **Architecture doc §7 and command-surface §3** assume CLI flags override template values
   generally. Corrected by §4: payload-level flags override, per-element flags are errors,
   matching the `--file` ruling.
2. **Command-surface §3.3** calls for `draw` to accept the union of every arg group.
   Corrected by §4.3: `draw` gains `--var` only, because the rest would be permanently
   inapplicable.
3. **Architecture doc §7** states a template deserializes directly into `DisplayElements`.
   Corrected by §2.1, and this one is a measurement rather than a judgment: the doc's own
   example template fails to parse, because `application_name` is required and templates must
   not carry it. A `TemplateFile` wrapper supplies the envelope and captures the
   `description` field the doc uses but `DisplayElements` silently discards. The claim that
   matters — `rectangle`, `countdown`, and `animation` for free — is unaffected and was
   verified end to end.
4. **This document's own §3.3** originally listed non-ASCII text as an offline `validate.rs`
   check producing a warning. Corrected in §3.3, and — like #3 — this is a measurement, not a
   judgment: busylib's `Text` validates ASCII at *deserialization*, so a non-ASCII character
   fails inside `toml::from_str::<TemplateFile>` before any `validate.rs` code runs; the branch
   was unreachable and was removed in Task 4. What shipped instead (Task 5) sanitizes `--var`
   and message values with `sanitize::to_ascii` before substitution, warning once per
   invocation — the same treatment `busy text` gives its message, and the case that actually
   matters given how often piped-in text (e.g. a commit subject) carries smart quotes. A
   literal non-ASCII character written into the template file itself still hard-fails, naming
   the template, which is unchanged and correct: the template author owns that file.

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

**Embedded examples grow the binary and can rot silently.** Every file under `templates/`
ships in every binary, so a large PNG committed there is paid for by every user. Text
templates are a few hundred bytes each and this is not a concern at the current scale; revisit
if an example ever carries a real image. The rot risk — a committed template that no longer
renders — is what `tests/examples.rs` (§8) exists to prevent, and that test is load-bearing
rather than a nicety.

**`undeclared_variables` is static analysis and can over-report.** A variable referenced only
inside a `{% if false %}` branch is still reported as required. Acceptable — it errs toward
asking for too much rather than rendering an empty string onto the panel — but it must be
documented in `template show`'s output so the list reads as "variables this template
mentions", not "variables you must supply".
