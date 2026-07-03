# Changelog

## Unreleased

### Information architecture

- Search scope is now honest and visible: the placeholder says what will be
  searched ("Search in <project>…" vs "Search all sessions…"), and results
  carry a sticky scope header — hit count, scope, one-click "Search all"
  widening, and an explicit exit. Previously a selected project silently
  narrowed the search while the box still promised "all sessions".
- New pinned **Recent** view above the project list: the latest sessions
  across every project, newest first, with the owning project on each row.
- Session meta lines lead with the (brighter) relative time — what the eye
  scans for in a recency-sorted list.
- The Resume button's tooltip now says what will happen per provider
  (terminal command vs opening the owning client) before the click.
- Search boxes share one style app-wide (the project filter and the session
  search are visually identical; placeholders carry the semantic difference).
- Agent filter reworked as a scope bar: an explicit [All] chip plus one chip
  per agent, additive — focusing on a single agent is one click (it was N−1
  under the previous subtractive model), further clicks add to the
  selection, and emptying it returns to All. Highlighted = active scope
  throughout.
- Settings: Language lives under an "Interface" group.

## 0.4.0 — 2026-07-03

- Self-update: the app checks GitHub Releases in the background after boot
  and shows a persistent banner when a new version is available — installs
  only on click, never silently, then relaunches. Manual check plus the
  current version live in Settings. Update packages are minisign-verified
  on top of Apple's signing and notarization. (Requires installing this
  version manually once; earlier versions have no updater.)

## 0.3.0 — 2026-07-03

- Sidebar project filter: live name search plus per-agent badge toggles.
  Subtractive model — every agent starts lit (lit = shown, solid color;
  unlit = excluded, hollow outline), so the visual state always matches the
  result; unlighting the last agent resets to all-lit. Display-layer only —
  discovery, global search and the CLI are unaffected.

## 0.2.0 — 2026-07-03

### Providers

- **Cursor** (experimental) — reads the IDE-side session store
  (`state.vscdb`, SQLite opened strictly read-only; ADR-7) with both composer
  generations supported (legacy inline conversations and the current
  bubble-store format). Projects derive from the workspace mapping; search
  goes through the provider-internal degrade path (parallel per composer).
  Resume opens the Cursor app at the project (new `LaunchMode::GuiApp` —
  no public deep link to a specific past chat exists yet).
- **Qoder** — full capability. Session pairs (`<id>-session.json` +
  `<id>.jsonl`) under `~/.qoder/projects/**`; metadata gives titles, token
  usage and resume/fork chain parents for free; subagent Task sessions fold
  under their root session. Full-text search runs on the grep engine; resume
  via `qodercli -r`.
- **Trae** — investigated and closed: AI sessions are stored server-side
  (only an input history exists locally), so there is nothing a file-based
  provider could read.

### App responsiveness

- Every store-touching Tauri command moved off the main thread
  (`async` + `spawn_blocking`) — clicks, hover and scrolling no longer freeze
  while a discovery or search runs.
- Stale-while-revalidate session cache in the frontend: clicking a project
  renders the last-seen list instantly and refreshes in the background; a
  startup warm-up (one whole-store discovery) makes even first clicks
  instant. Summaries are cached per `id@mtime`, so only changed sessions
  re-parse. Presentation-layer memory only — the backend stays stateless.
- Timeline render chunk reduced 200 → 80 messages per frame; slow loads
  (>150ms) show an explicit loading state; project/session/search fetches
  are generation-guarded against superseded clicks.

### UI

- Distinct badge colors for all five providers (Cursor purple, Qoder amber) —
  previously both fell back to the same gray.
- Interface internationalization: English and 中文, hand-rolled dictionary
  (no build chain). Language setting in the settings pane — Auto (system) /
  English / 中文 — persisted in config.json; the native "Settings…" menu item
  localizes on an explicit choice. Backend-produced messages (errors, resume
  notes) remain English.

### Settings

- Native menu-bar entry: "Settings…" (⌘,) in the app menu, inserted at the
  standard macOS position; the sidebar ⚙ button remains as an in-window
  mirror.
- Settings pane (⚙ in the sidebar) organized along engineering-tool
  dimensions: **Data sources** (per-provider enable switch and store-root
  override with live path validation), **Resume** (terminal choice, moved out
  of the sidebar), **Workspace** (hidden-project management), and the config
  file itself (path + Reveal in Finder).
- Config stays a single hand-editable JSON file
  (`~/.config/vibetrail/config.json`); unknown keys survive save round-trips,
  and a missing or broken file degrades to defaults.
- Provider discovery settings are honored by the CLI and the GUI alike:
  both shells now build their store from the same config (core-owned).
- New `vibetrail config [--json]` command prints the effective
  configuration — config path plus each provider's enabled state, effective
  root, and whether the root exists; the JSON shape is snapshot-pinned.

## 0.1.0 — 2026-07-02

Initial release: browse, search, and resume coding-agent sessions across
providers, from a Tauri app and a `vibetrail` CLI.

### Providers

- **Claude Code** — full capability: five-stage parse pipeline (message-id
  regrouping, UUID dedup, whitelist filtering, parent-child tree with
  fixed-order subagent merge), resume via `claude --resume`, deduplicated
  token stats, subagent tree view.
- **Codex** — rollout parsing with `response_item` whitelist (`event_msg`
  mirrors are skipped — trusting both double-counts messages), `.jsonl.zst`
  support, resume via `codex resume`.
- **Antigravity** (experimental) — transcript step whitelist, markdown
  artifacts (plan/task/walkthrough), heuristic project derivation from
  touched file paths; not resumable.

### Features

- Cross-provider project overview grouped by normalized cwd; orphaned paths
  flagged.
- Session list with continuous loading (50/page), copyable session-id chips,
  and resume/fork chains folded under their root session (Claude Code
  resume-forks, Codex forks and multi-agent worker threads).
- Timeline with markdown rendering (sanitized), collapsed tool/result/
  thinking blocks, 2000-char tool-result previews with load-full-on-demand,
  chunked lazy rendering.
- Full-text search built on the ripgrep engine crates (no external `rg`):
  parallel across providers and files, project scoping, 500-hit circuit
  breaker, per-message jump anchors; results stay open while browsing hits.
- One-click resume into Terminal.app, iTerm2, Ghostty, or Warp. Ghostty is
  driven through its Finder service (new tab in the project directory) rather
  than its preview scripting API, which proved unstable under load.
- Agent lettermark badges (CC / CX / AG) with full-name tooltips; copyable
  session-id chips in lists and the detail header.
- Everything scriptable: every query command takes `--json` with a
  snapshot-pinned schema.

### Performance (measured on a 20k-session / 3.4GB machine, release build)

- Project overview ~1.4s cold, session open 0.06s, first session page ~0.7s
  on a 717-session project, common-word search 0.1s, rare-word full-store
  search at ripgrep parity.
