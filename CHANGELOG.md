# Changelog

All notable changes to VaultOne are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[1.2.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.2.0
[1.1.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.1.0
[1.0.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.0.0
