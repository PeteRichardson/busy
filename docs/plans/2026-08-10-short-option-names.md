# Short option names

**Status: implemented on `main` (2026-08-10).** The mapping below shipped exactly
as specified, with one correction noted under Globals.

The blocking condition is resolved: the foundation plan finished, merged, and the
`worktree-busy-cli-foundation` worktree and branch were removed. `main` now holds
the whole linear history, and `for_review` was deleted, so the branch guidance in
the original draft no longer applies — this landed on `main` directly.

## Mapping

Per-command flags:

| Short | Long | Group |
|---|---|---|
| `-c` | `--color` | Style |
| `-f` | `--font` | Style |
| `-x` `-y` | `--x` `--y` | Placement (already shipped) |
| `-a` | `--align` | Placement |
| `-s` | `--screen` | Placement |
| `-w` | `--width` | Scrolling |
| `-r` | `--scroll-rate` | Scrolling |
| `-p` | `--priority` | Delivery |
| `-t` | `--timeout` | Delivery |
| `-u` | `--until` | Delivery |
| `-l` | `--led` | Delivery |
| `-i` | `--id` | Delivery |
| `-k` | `--keep` | Delivery |

Globals:

| Short | Long |
|---|---|
| `-n` | `--dry-run` |
| `-j` | `--json` |
| `-q` | `--quiet` (already shipped) |

**Correction to this draft:** it listed `-v`/`--verbose` as already shipped. It is
not — the foundation plan's whole-branch review found `--verbose` was a declared,
help-documented no-op producing byte-identical output, and removed it rather than
ship a flag that does nothing. `-v` is therefore free, alongside the letters listed
below.

Long-only, deliberately: `--addr`, `--app`, `--token`, `--http-timeout`,
`--api-prefix`, `--scroll-start-delay`, `--scroll-repeat-delay`.

Unavailable: `-h` and `-V` (clap), `-m` (forbidden by a Global Constraint and by
`tests/cli_surface.rs::there_is_no_message_flag`).

Still free afterwards: `-b -d -e -g -o -v -z` and every capital.

## Rationale for the contested letters

- **`-t` → `--timeout`, not `--token` or `--http-timeout`.** Frequency: the
  element timeout is typed per-invocation; the other two are set once. `--token`
  is additionally better off long-only — a short flag invites
  `busy -t hunter2` into shell history and `ps`, which the foundation plan's
  Global Constraints explicitly warn against.
- **`-a` → `--align`, not `--addr`/`--app`/`--api-prefix`.** Align is a
  per-invocation placement flag that reads naturally beside `-x`/`-y`. The others
  are env-backed (`BUSY_ADDR`, `BUSY_APP`) and config-file-backed, so they are
  typed rarely.
- **`-s` → `--screen`, not `--scroll-rate`.** Front vs back is a routine per-call
  choice; scroll rate is tuned once. `-r` for "rate" is a clean second letter.
- **`-c` → `--color`,** accepting that this forecloses the conventional `-c` for
  a future `--config`. There is no `--config` flag today (the path is fixed at
  `~/.config/busy/config.toml`); if one lands it can go long-only or take `-C`.
- **No shorts for the connection globals.** They are typed rarely, and a global
  short is reserved across every present and future subcommand — an expensive
  commitment for Phase 3 (`draw`, `asset`) and Phase 4 (templates).
- **No shorts for the two scroll delays.** Rare, and no memorable letter;
  `-S`/`-R` would be noise.

## Implementation notes

Most fields can use a bare `short` — clap derives the first letter of the field
name, which is already the letter chosen. Two need an explicit letter:

- `dry_run` would derive `-d`; it needs `short = 'n'`.
- `scroll_rate` would derive `-s`, taken by `--screen`; it needs `short = 'r'`.

Clap catches duplicate shorts with a `debug_assert`, so verify with `cargo test`
in debug rather than a release build — and the assert only fires when the owning
subcommand is actually parsed, which `tests/cli_surface.rs` and `tests/payload.rs`
both do.

`--timeout` is `u32`, so `-t` taking a value costs nothing; `-x`/`-y` keep their
`allow_negative_numbers`.

## Ship it as one commit

Three files have to move together or the docs will describe half the surface:

- `src/cli.rs` — the mapping.
- `README.md` — created by Task 12 with long-form examples taken verbatim from
  the foundation plan. Update the example block to show short forms.
- `tests/cli_surface.rs` — add a case proving a short form reaches the payload,
  e.g. `busy -n text -c red -f small "hi"` matching the `--color red --font
  small` snapshot. This pins the mapping against an accidental future reshuffle.

Settle this before Phase 5 ships `clap_complete` completions: after that, and
after the README is published, changing a short name is a breaking change.
