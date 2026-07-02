<p align="center">
  <img src="assets/logo.png" alt="VibeTrail" width="128" height="128" />
</p>

# VibeTrail

**Session browser & resume for coding agents (Claude Code, Codex, Antigravity, …)**

Browse and search every coding-agent session you have ever run — across all
projects and all agents — and jump back into any of them with one click.

> **Unofficial.** VibeTrail is an independent open-source project. It is not
> affiliated with, endorsed by, or sponsored by Anthropic, OpenAI, Google, or
> any other agent vendor. It only reads the session files those tools already
> store on your machine, strictly read-only.

## Why

Coding agents scatter session history across private local directories and
formats. Their built-in resume pickers show only a handful of recent sessions,
there is no cross-project view, no cross-agent view, and no way to answer
"which session did I fix that nginx config in three weeks ago?". VibeTrail
closes the loop: **browse → search → resume**.

- **One inbox for every agent.** A provider abstraction unifies session
  history behind a single UI and CLI.
- **Lightweight by design.** No database, no index, no background process, no
  file watcher. Files are read live; search links the ripgrep engine crates
  directly.
- **Two entry points.** A macOS app (Tauri) and a `vibetrail` CLI (JSON
  output for scripting).

## Status

| Provider | Status | Capabilities |
|----------|--------|--------------|
| Claude Code | ✅ | browse / search / resume, subagent view, token stats |
| Codex | ✅ | browse / search / resume, incl. `.jsonl.zst` |
| Antigravity | ✅ experimental | browse / search / artifacts (no resume) |

## Requirements

- macOS 14+ (primary target; the core and CLI are portable Rust)
- Rust toolchain (to build from source)

## Build

```sh
# CLI
cargo build --release -p vibetrail-cli
target/release/vibetrail --help

# GUI (Tauri v2)
cargo run --release -p vibetrail-app
```

Search is built in (the ripgrep engine crates are linked directly) — no
external `rg` binary required.

## CLI

```sh
vibetrail projects                      # every project, across all agents
vibetrail sessions <project> [-n 20]    # sessions of one project, newest first
vibetrail search "race condition"       # full-text search, grouped by session
vibetrail show <session-id>             # outline view (--full for transcript)
vibetrail resume <session-id>           # cd to the project and exec the agent's resume
vibetrail open [<project>]              # launch the GUI
```

Every query command accepts `--json`. Session ids accept any unique prefix.

Exit codes: `0` success · `1` usage error · `2` data error · `3` resume
precondition failed · `4` operation unsupported by provider.

## Configuration

The GUI's Resume button can target Terminal.app (default), iTerm2, Ghostty, or
Warp — pick one in the sidebar selector (stored in
`~/.config/vibetrail/config.json`). Warp cannot be scripted to run a command,
so VibeTrail opens the project there and puts the resume command on your
clipboard.

## Safety

- Agent storage directories (`~/.claude`, `~/.codex`, `~/.gemini`, …) are
  opened strictly read-only.
- Resume validates that the project path still exists before launching
  anything.
- The GUI's resume uses macOS Automation (osascript → Terminal); you will be
  asked for permission on first use.

## Contributing

Provider implementations are self-contained under
`crates/vibetrail-core/src/providers/`. New providers are welcome — the bar is
that all capabilities must work by reading files only (no host process, no
reverse-engineered binary formats). See `TECH_SPEC.md` for the provider
protocol and parsing rules.

## License

[MIT](LICENSE)
