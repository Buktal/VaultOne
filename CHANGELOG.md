# Changelog

All notable changes to VaultOne are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.3.0] - 2026-07-28

### Added

- **Multi-CLI usage collection** — usage collection now spans four AI CLIs that write to local logs: Claude Code (`~/.claude`), Codex (`~/.codex`), Gemini CLI (`~/.gemini`), and OpenCode (SQLite). Each source's token semantics are normalized into the same four-bucket model: Codex's cache-inclusive input becomes fresh input, Gemini's `thoughts` fold into output, OpenCode's `cache.write` maps to cache creation. Claude Code dedup now picks the best message-id snapshot (one with `stop_reason` set, else the largest output), so message_start snapshots no longer freeze and undercount output. Seed pricing added for gpt-5.5 / 5.4 / 5.2 (prefix-fallback covers `-codex` variants) and DeepSeek v3.x.
- **Device-scoped usage tracking** — filter the dashboard, the expanded lightweight card, and the tucked mini-bar by device. A unified device picker drives all three windows; the tucked bar's hover drawer opens on hover and lists devices on click.
- **Device lifecycle** — forget a peer device locally (clears its rows and artifact dir); stale peers with no git presence auto-clear within ~30s on both sync pull and the collect path, while a still-active peer self-heals on the next sync. Recent requests and the logs' Device column now show device names.
- **Persistent usage filter** — the time-range / model / device-scope filter survives app restarts.
- **Per-shape window geometry** — full, expanded, and tucked each remember their own placement and state, so switching shapes restores the last position instead of resetting.
- **System-proxy sync** — libgit2 push/fetch/clone/connect now follows the OS system proxy (env vars, then the Windows registry), so Synced-mode clients behind Clash/Mihomo or a corporate gateway no longer time out silently. Proxy changes apply on the next sync — nothing is cached.

### Changed

- **UI polish** — title bar raised to 36px; mid-window device selector tightened. Token columns carry a language-neutral `tok` unit in the header. Select popovers auto-fit content (never narrower than the trigger) and open top-aligned, fixing the jumping model dropdown.

### Fixed

- **Linux tray** — added a "Show" entry so the window can be restored from the tray on Linux, where the libappindicator/SNI backend never emits tray click events and left-clicking was a no-op. Windows/macOS left-click restore is unchanged.
- **Pricing seed** — a malformed seed literal now panics at startup instead of silently returning 0 and skewing every cost calculation.
- **Today trend** — the "today" trend now spans the full day instead of stopping at the current hour.
- **Request log** — switching the filter resets the log page, so you don't land on a stale, out-of-range page.

## [1.2.0] - 2026-07-24

### Added

- **Lightweight glance mode** — the main window morphs into a small, always-on-top "today" snapshot docked to the right screen edge. Two shapes reachable from one another: a tucked mini-bar that always shows today's token total, and an expanded card mirroring the dashboard's anchor. Switch full ⇄ expanded ⇄ tucked from any shape.
- **Multi-skin theming** — recolor the accent and chart palette across five skins (Neutral, Sage, Azure, Crimson, Mauve); Neutral (greyscale chrome) is the new default. Per-device, never synced.

### Changed

- **Usage trend** — the trend chart is now multi-line with data points instead of a single line, so each metric reads on its own.

### Fixed

- **Lightweight mode** — the entire tucked bar is draggable now (not just a tiny corner grip), and a press still distinguishes click-to-expand from drag-to-move.

## [1.1.0] - 2026-07-23

### Added

- **Auto-update** — check for new versions on launch (throttled to once per 24h) or manually from Settings; download and install signed installers straight from GitHub Releases, with Ed25519 signature verification and one-click relaunch. Distributed entirely through GitHub — no self-hosted server. On updater failure, a manual fallback opens the Releases page.
- **Display language** — switch the UI between English, 简体中文, and 日本語.

### Fixed

- **Lightweight mode** — edge-flush the tucked peek icon and smooth out the diagonal reveal animation.

## [1.0.0] - 2026-07-23

First public, open-source release.

### Added

- **Dashboard** — four-bucket token consumption (input / output / cache creation / cache read), cache-hit rate (`cache_read / (input + cache_creation + cache_read)`), total requests and total cost (USD, frozen at collection), dual-axis token-vs-cost usage trends, per-call request log (model, token breakdown, cost, turn duration, `stop_reason` / `service_tier` chips), and per-turn cost and wall-clock views.
- **Collection** — read-only parsing of Claude Code session logs (source logs are never modified), cursor-based incremental scan, tray-resident background scheduler. Pluggable provider architecture (Claude Code today, more planned).
- **Sync (optional)** — Standalone mode (full dashboard, zero network) and Synced mode (align usage across devices through a GitHub repository you own); plain-text artifacts partitioned by device and date (`data/<device>/usage-YYYY-MM-DD.jsonl`).
- **Cost & pricing** — editable per-model pricing overrides; rebill for records that had no price when collected, without re-costing existing history.
- **Experience** — lightweight glance mode (edge-tuck + hover-to-peek today's usage), custom title bar, light / dark theme, local-first and private by default.
- **Packaging** — cross-platform installers for Windows, macOS (Apple Silicon), and Linux, built automatically on tag push via GitHub Actions.

### Known limitations

- **macOS**: Apple Silicon (arm64) only; builds are unsigned — right-click → **Open** on first launch (or `xattr -dr com.apple.quarantine /Applications/VaultOne.app`). Intel Mac users can build from source.
- **Providers**: Claude Code only; additional providers (Codex, Cursor, …) are planned.

[Unreleased]: https://github.com/Buktal/VaultOne/compare/v1.3.0...HEAD
[1.3.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.3.0
[1.2.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.2.0
[1.1.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.1.0
[1.0.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.0.0
