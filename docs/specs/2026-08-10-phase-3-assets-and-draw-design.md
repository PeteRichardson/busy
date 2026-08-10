# Phase 3 — assets, images, and `busy draw`

**Design spec, 2026-08-10**

Phase 3 of `docs/busy-cli-architecture.md`. Phases 1–2 shipped and merged: `busy text`
draws to the bar with layered configuration, `--dry-run`/`--json`/`--quiet`, stdin,
replace-by-default, and `busy clear`.

The command surface this phase implements was settled in
`docs/specs/2026-08-09-busy-cli-ux-design.md` §3 and §5 — `busy image` folded into a
polymorphic `busy draw`, images kept two-step, asset sync device-authoritative. This
document records what Phase 3 adds, and the hardware measurements that changed its
shape.

The architecture doc is the authority on *what the device does*; the command-surface
spec on *what the user types*. This document is the authority on *how Phase 3 is built*.

---

## 1. Measured on hardware — what the device does with images

Run against a physical bar (API 25.0.0) on 2026-08-10, uploading generated PNGs and
reading frames back through `GET /screen`.

| Input | `assets/upload` | `display/draw` | Result |
|---|---|---|---|
| PNG within the panel | 200 | 200 | renders at its natural size |
| PNG larger than the panel | 200 | **200** | **silently cropped** to the top-left region |
| JPEG | **200** | 400 | `Failed to decode image …` |
| GIF | **200** | 400 | `Failed to decode image …` |
| Colour PNG on the back panel | 200 | 200 | device does its own colour → greyscale |

Three consequences, each of which moves work into or out of this phase:

**The device decodes PNG natively, so we do not need its `.image` format.** The
architecture doc's §7 instruction to "re-encode images for the target display" overstates
the job — stock assets happen to be `.image` files, but user assets are plain PNG and
draw fine. We produce PNG, nothing more exotic.

**The device crops rather than scales, and reports success while doing it.** A 200×100
logo renders as its top-left 72×16 corner with a `200 OK`. Verified by the geometry: the
test image's diagonal advances two x-pixels per y-pixel on the panel, which is the source
slope preserved — a crop, not a fit. This is the same silent-failure class as the text
clipping §8.3 of the command-surface spec already warns about, and it is the reason
resizing is the load-bearing part of this phase.

**`assets/upload` is a dumb byte write.** It accepts a JPEG happily; the failure surfaces
later, in a different command, as a device error naming an `/ext` path. That seam is bad
enough to determine where conversion belongs (§3).

`scripts/probe-device.sh` should gain assertions for the crop and the PNG-only decode, so
a firmware change to either is caught rather than silently altering behaviour.

---

## 2. Command surface

```sh
busy asset upload ./logo.png               # fit to the front panel, store as logo.png
busy asset upload ./logo.jpg --screen back # fit to 160x80, converted to PNG
busy asset list
busy asset delete                          # confirms; --yes for CI

busy draw logo.png                         # an uploaded asset
busy draw logo.png --opacity 50
busy draw shared/checkmark_front_8x8.image # a device built-in
busy draw --file payload.json              # raw DisplayElements
```

`draw` takes the Placement and Delivery arg groups already shared by `text`, plus
`--opacity`. Short options follow `docs/plans/2026-08-10-short-option-names.md`; `-o`
is free and is the natural short for `--opacity`.

---

## 3. Conversion happens at upload

**Decision: convert at `asset upload`, and rename the output to `.png`.**

Uploading raw bytes and letting the draw fail puts the error in the wrong command. The
user ran `busy asset upload ./photo.jpg`, it succeeded, and then a *later* command fails
with a device message about `/ext/user_assets/busy/photo.jpg`. Converting at upload moves
the failure to the command holding the bytes and the user's attention, and guarantees
that whatever reaches the device is drawable.

The cost is that the remote name differs from the local one: `logo.jpg` is stored as
`logo.png`. That is accepted, and warned about, because the alternative — storing PNG
bytes under a `.jpg` name — produces a file whose extension lies, which will mislead
anyone reading `storage/list` output or debugging on the device.

### 3.1 The pipeline

`src/image.rs`, the only module that imports the `image` crate:

```
read bytes -> decode -> if larger than target, scale down preserving aspect
           -> encode PNG -> (bytes, original_dimensions, final_dimensions)
```

- **Never enlarges.** An 8×8 icon stays 8×8 rather than being blown up to fill the panel.
- **Returns both dimension pairs** so the caller can report "resized 200×100 → 72×16"
  rather than silently changing the file. A resize that the user cannot see is the
  problem we are fixing, not a feature.
- **Fit, not fill.** Scale so the whole image is visible, letterboxing rather than
  cropping. Cropping is precisely the device behaviour we are protecting against.

Dependency: `image` with `default-features = false` and only `png`, `jpeg`, `gif`
enabled — decode those three, encode PNG only. This keeps the tree as small as the crate
allows.

### 3.2 The fit target is the selected panel

`--screen front|back` on `asset upload` chooses the target: 72×16 or 160×80, defaulting
to `front`. An asset is therefore panel-specific, which is honest — a 72×16 image
genuinely is a front-panel asset. Drawing it on the other panel still works; it is merely
small (front asset on the back) or cropped by the device (back asset on the front).

`--fit WxH` was considered and deferred. Phase 4's `error` template puts an icon *beside*
text, occupying part of the panel rather than all of it, so arbitrary dimensions will be
wanted then. Adding the flag now, with no caller, is speculative; adding it in Phase 4
alongside its first real use is not.

**`--screen` carries two related meanings, and both are intended.** On `asset upload` it
selects the panel an image is *fitted for*; on `draw` (and `text`) it selects the panel an
element is *rendered on*. The unifying reading is "this operation concerns the front/back
panel", which holds in both cases — but they are separate flags on separate subcommands,
and using one does not imply the other:

```sh
busy asset upload ./logo.png --screen back   # fitted to 160x80
busy draw logo.png                           # drawn on the FRONT — device crops it
busy draw logo.png --screen back             # what was meant
```

The second line is a real hazard, not a hypothetical: the device crops without complaint
(§1). `draw` should warn when an asset was plainly fitted for the other panel — see the
dimension-check risk in §10 — and until that exists, `--help` for `asset upload --screen`
must say that the panel choice has to be repeated at draw time.

### 3.3 Naming

The stored name is the local file stem plus `.png`. It is validated against `AssetName`'s
`^[a-zA-Z0-9._-]+$` **before** any bytes are sent, so an unusable name fails locally with
a clear message rather than as a device rejection. A name that cannot be made valid is a
usage error naming the offending characters.

### 3.4 No content-hash skip-if-present

The architecture doc lists it for this phase. Dropped, deliberately: `busy asset upload`
is an explicit command, the device overwrites in place so re-uploading is harmless, and
silently skipping what the user asked for is more surprising than doing it. Content
hashing exists to make *automatic* template asset sync cheap — that is Phase 4's problem,
and §5.5 of the command-surface spec already describes the device-authoritative mechanism
for when there is a caller that needs it.

---

## 4. `busy draw`

### 4.1 Resolution, Phase 3 subset

1. `<name>` begins with `shared/` → `stock_path`
2. *(a local template directory → template — **Phase 4**, absent here)*
3. otherwise → an app asset path → a single `ImageElement`

The only difference from the command-surface spec's §3.1 is the missing middle rule. The
implementation should be shaped so Phase 4 inserts rule 2 rather than restructures the
function.

`--as image|stock` forces the interpretation. `--file` and the positional are mutually
exclusive, enforced by a clap arg group, and supplying neither is an error. A second
positional is a hard error — images take no message — which is the guard that keeps a
typo'd name from becoming a silently doomed asset draw once templates exist.

### 4.2 `--file`

Deserialize the file directly into `busylib`'s `DisplayElements` and draw it, giving
parity with upstream `busybar assets draw --file`. It needs no template machinery, so it
lands here. Delivery flags still apply as overrides; a deserialization failure is a usage
error quoting the serde message and the offending path.

### 4.3 `--id`

Defaults to `image` for an asset or stock draw, matching the `--id` table in the
command-surface spec §3.4. For `--file` it is an error: element ids come from the payload.

---

## 5. `asset list` and `asset delete`

**`list`** reads `/ext/user_assets/<app>/` through `storage/list` and prints name and
size. A `400` means the directory does not exist, which happens because delete-all removes
the directory rather than emptying it — render that as "no assets", never as an error.

**`delete`** is all-or-nothing for an app; the API has no per-file delete (§5.2 of the
command-surface spec, measured). It therefore lists first and shows the files it is about
to destroy, so the blast radius is concrete, then confirms on a tty. `--yes` skips the
prompt for CI. When stdin is not a tty and `--yes` was not given, it refuses rather than
either prompting into the void or destroying silently.

---

## 6. Structure

```
src/
├── image.rs          # NEW — decode, fit, encode. The only importer of the `image` crate.
├── cmd/
│   ├── asset.rs      # NEW — upload | list | delete
│   └── draw.rs       # NEW — resolution + payload construction
└── device.rs         # gains upload/list_assets/delete_assets + image type re-exports
```

`device.rs` remains the only module naming `busylib`, and `image.rs` becomes the
equivalent boundary for the `image` crate. Both exist for the same reason: one file to fix
when an upstream layout moves.

**One targeted refactor.** The panel dimensions (72×16, 160×80) are currently private
constants in `validate.rs`, and `image.rs` needs them for the fit target. They move beside
`config::Defaults::position(screen)`, which already encodes the same geometry. Leaving
them in two files guarantees drift.

---

## 7. Errors

Reuse `CliError`; the exit-code contract is unchanged (0 success, 1 runtime, 2 usage).
New cases, all usage errors:

- an input format we cannot decode — name the formats we do accept
- a local file that does not exist or cannot be read
- an asset name that cannot be made valid, naming the offending characters
- a `--file` payload that fails to deserialize into `DisplayElements`
- `--as` naming an interpretation the resolved input cannot satisfy

Device errors keep flowing through `map_error`, which already turns 409 into priority
guidance and surfaces the device's own 400 text — the latter matters here, since
`Failed to decode image …` is exactly what a stale or hand-uploaded asset produces.

---

## 8. Testing

- **`src/image.rs` unit tests:** never enlarges; aspect preserved on both landscape and
  portrait sources; an exactly-panel-sized image passes through unchanged; each accepted
  input format decodes; an unsupported format produces the usage error.
- **`tests/asset.rs`:** wiremock upload/list/delete, including the 400-means-empty list,
  the `--yes` path, and the refusal when stdin is not a tty and `--yes` is absent.
- **`tests/draw.rs`:** resolution order, `--as` override, second-positional error,
  `--file` round trip, `--opacity` reaching the payload.
- **Golden snapshots** for image payloads via `--dry-run`, as for text.
- **One real-device verification** in the final task: upload an oversized PNG, confirm the
  CLI resized it, draw it, and read the frame back to prove the whole image is on the
  panel rather than a cropped corner.

The unit tests are the important ones: the fit maths is the only genuinely new logic in
this phase, and it is pure.

---

## 9. Out of scope

Templates and `--var`; the inert-flag check and `--id`-is-an-error-for-templates, both of
which need templates to exist; content-hash sync; `animation`, `rectangle`, and
`countdown` verbs; the replace-by-default flicker fix; and the fourth invisibility mode
(command-surface spec §9 items 1 and 2). All remain recorded there.

---

## 10. Risks

**`image` is a heavy dependency with a slow cold compile,** and it is the one place this
phase meaningfully grows the build. Hand-rolling PNG was considered — a minimal encoder
is genuinely small, as the probe scripts demonstrated — but decoding arbitrary real-world
PNGs, and resampling with acceptable quality, is not the same problem as emitting a
synthetic test image. Accepted, with features minimised.

**Panel-specific assets may surprise.** An asset fitted to the front panel is small on the
back. The CLI cannot resize server-side, so the honest mitigation is a warning at draw
time when an asset's dimensions look wrong for the target panel. That requires reading the
asset back via `storage/read` to learn its dimensions, which costs a round trip per draw —
deferred unless it proves to be a real annoyance in use.
