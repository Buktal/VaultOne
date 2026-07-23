# 更新日志

VaultOne 的所有显著变更记录于此。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，并遵循[语义化版本](https://semver.org/lang/zh-CN/)。

## [1.1.0] - 2026-07-23

### 新增

- **自动更新** —— 启动时检查新版本（24h 节流）或从设置手动检查；从 GitHub Releases 直接下载并安装签名安装包，带 Ed25519 签名校验与一键重启。完全通过 GitHub 分发——无需自建服务器。更新失败时提供跳转 Releases 页面的手动降级。
- **显示语言** —— UI 在 English、简体中文、日本語 之间切换。

### 修复

- **轻量模式** —— 修正贴边速览图标的边缘吸附，并平滑斜向展开动画。

## [1.0.0] - 2026-07-23

首次公开发布（开源）。

### 新增

- **看板** —— 四桶 token 消耗（input / output / cache creation / cache read）、缓存命中率（`cache_read / (input + cache_creation + cache_read)`）、总请求数与总成本（USD，采集入库时冻结）、双 Y 轴 token 对成本的使用趋势图、Per-call 请求日志（模型、token 明细、成本、回合时长、`stop_reason` / `service_tier` 语义标签）、以及 Per-turn 的成本与墙钟耗时视角。
- **采集** —— 只读解析 Claude Code 会话日志（绝不修改源日志）、基于游标的增量扫描、托盘常驻后台调度器。可插拔的 provider 架构（当前 Claude Code，更多规划中）。
- **同步（可选）** —— 单机模式（完整看板，零网络）与同步模式（通过你掌控的 GitHub 仓库在多设备间对齐用量）；纯文本产物按设备与日期切分（`data/<device>/usage-YYYY-MM-DD.jsonl`）。
- **成本与定价** —— 可编辑的 per-model 定价覆盖；对采集时缺价的记录回算，不重算已有历史。
- **交互** —— 轻量速览模式（贴边缩成半图标 + 悬停瞥见今日用量）、自定义标题栏、浅色 / 深色主题、本地优先且默认私密。
- **打包** —— Windows、macOS（Apple Silicon）、Linux 跨平台安装包，打 tag 后由 GitHub Actions 自动构建。

### 已知限制

- **macOS**：仅 Apple Silicon（arm64）；构建未签名——首次启动右键 →「打开」（或 `xattr -dr com.apple.quarantine /Applications/VaultOne.app`）。Intel Mac 用户可从源码构建。
- **Provider**：当前仅 Claude Code；更多 provider（Codex、Cursor 等）规划中。

[1.1.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.1.0
[1.0.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.0.0
