# Changelog

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
- One-click resume into Terminal.app, iTerm2, Ghostty, or Warp (degraded:
  opens at path + command on clipboard).
- Everything scriptable: every query command takes `--json` with a
  snapshot-pinned schema.

### Performance (measured on a 20k-session / 3.4GB machine, release build)

- Project overview ~1.4s cold, session open 0.06s, first session page ~0.7s
  on a 717-session project, common-word search 0.1s, rare-word full-store
  search at ripgrep parity.
