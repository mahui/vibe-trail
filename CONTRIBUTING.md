# Contributing to VibeTrail

Thanks for your interest! The most valuable contribution is a **new provider**
— support for another coding agent's session store. This guide is mostly about
that; general fixes follow the same workflow rules at the bottom.

## Ground rules (from the architecture ADRs)

1. **Files only.** A provider may only read files on disk. Capabilities that
   need a live host process (language servers, IPC) or reverse-engineering
   schema-less binary formats are out of scope — degrade or skip them
   (ADR-6). Agent store directories are opened strictly **read-only**.
2. **No index, no cache, no daemon.** Every query re-reads the store (ADR-2).
   Use `rayon` for parallel I/O instead of caching.
3. **Provider isolation.** Providers must not depend on each other, and
   provider-specific knowledge must not leak into the generic layer. If the
   engine needs to understand your format, add a hook to the `Provider` trait
   with a sane default instead.
4. **Projects are derived, not stored.** Extract a cwd from session metadata;
   the store groups by normalized path. No provider-specific grouping UI.

## Implementing a provider

Create `crates/vibetrail-core/src/providers/<your_agent>/` and implement
`Provider` (see `crates/vibetrail-core/src/provider.rs` and TECH_SPEC §3):

| Method | Contract |
|--------|----------|
| `discover()` | Metadata-level only: directory listing + at most the first line/block per file. Tens of thousands of files must stay fast — parallelize. |
| `parse()` | Full transcript → unified `Session`. Unknown entry types are **counted, never fatal** — session formats drift with every agent release. Truncate tool results at 2000 chars for display. |
| `message_full()` | Untruncated single message, re-read on demand. |
| `find()` | Locate by native id/prefix. Override if ids are encoded in file names — opening one session must not pay a whole-store scan. |
| `quick_title()` | Bounded head/tail read for the project overview. Never full-parse here. |
| `resume_spec()` | Metadata-only; `None` if the agent has no CLI resume. |
| `search_roots()` / `resolve_hit()` | Feed the grep engine and map matched lines back to a session/message anchor. |
| `parent_native_id` (on `RawSession`) | Resume/fork chain parent, if your format records one — extract it from bytes discovery already reads. |

Study the three existing providers first; they were deliberately kept
different where the formats differ (Claude Code's five-stage pipeline vs
Codex's linear whitelist) and identical where they don't.

### Tests are the admission ticket

Every provider needs **fixture parity tests** (`crates/vibetrail-core/tests/`):
hand-crafted sample files covering the format's real quirks — streaming
splits, duplicate ids, unknown entry types, compressed variants — with
assertions pinned to hand-counted expected values. This is the regression
line against format drift. Run everything with:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

## General workflow

- Conventional commits, English commit messages.
- Parsing changes require the corresponding fixture tests to pass first;
  new CLI output fields need `--json` snapshot updates (deliberate ones).
- The Tauri frontend stays static HTML/CSS/JS — no Node toolchain. Vendored
  JS libraries are pinned files under `apps/vibetrail-app/ui/vendor/`.
- No agent vendor brand assets anywhere (names, logos, icons).
- App icons must follow the Apple icon grid (content 80.5% of canvas,
  ~9.8% transparent margins) or they render oversized in the macOS Dock.
