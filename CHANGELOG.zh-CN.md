# 更新日志

VaultOne 的所有显著变更记录于此。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，并遵循[语义化版本](https://semver.org/lang/zh-CN/)。

## [Unreleased]

## [1.3.0] - 2026-07-28

### 新增

- **多 CLI 用量采集** —— 用量采集现已覆盖四个会向本地写日志的 AI CLI：Claude Code（`~/.claude`）、Codex（`~/.codex`）、Gemini CLI（`~/.gemini`）与 OpenCode（SQLite）。各来源的 token 口径被归一到同一套四桶模型：Codex 含缓存的 input 折算为 fresh input、Gemini 的 `thoughts` 并入 output、OpenCode 的 `cache.write` 映射为 cache creation。Claude Code 去重改为选取最优 message-id 快照（优先取带 `stop_reason` 的，否则取 output 最大者），不再因 message_start 快照冻结而少算 output。新增 gpt-5.5 / 5.4 / 5.2（前缀回退覆盖 `-codex` 变体）与 DeepSeek v3.x 的定价 seed。
- **设备作用域用量跟踪** —— 看板、轻量展开卡、贴边迷你条三处窗口均可按设备筛选。统一设备选择器驱动全部三个窗口；迷你条的悬停抽屉在悬停时展开、点击时列出设备。
- **设备生命周期** —— 可在本地遗忘对端设备（清掉其用量行与产物目录）；失联（在 git 中无存在）的对端设备在 sync pull 与 collect 路径上约 30s 内自动清除，而仍活跃的对端会在下次 sync 时自愈。最近请求与日志的设备列现在显示设备名。
- **用量过滤器持久化** —— 时间范围 / 模型 / 设备作用域过滤器跨应用重启保留。
- **每形态窗口几何记忆** —— full、expanded、tucked 三种形态各自记忆位置与状态，切换形态时恢复上次位置，而非重置。
- **同步跟随系统代理** —— libgit2 的 push/fetch/clone/connect 现跟随 OS 系统代理（先环境变量，再 Windows 注册表），Clash/Mihomo 或企业网关后的同步客户端不再静默超时。代理变更在下次 sync 即生效，不做缓存。

### 变更

- **界面打磨** —— 标题栏提到 36px，收紧中部设备选择器。token 列头使用语言中立的 `tok` 单位。下拉浮层自适应内容（绝不窄于触发器）且默认顶部对齐，修掉了模型下拉跳动的问题。

### 修复

- **Linux 托盘** —— 新增「Show」菜单项，使窗口可从托盘恢复：libappindicator/SNI 后端在 Linux 上从不触发托盘点击事件，左键原先无效。Windows/macOS 的左键恢复不变。
- **定价 seed** —— 损坏的 seed 字面量现在会在启动时 panic，而非静默返回 0 导致所有成本计算偏斜。
- **今日趋势** —— 「今日」趋势现在覆盖整天，而非停在当前小时。
- **请求日志** —— 切换过滤器时重置日志分页，避免落到过时、超出范围的页上。

## [1.2.0] - 2026-07-24

### 新增

- **轻量速览模式** —— 主窗口可变身为贴在屏幕右缘、常置顶的「今日」快照。两种形态可互通：贴边迷你条常显今日 token 总数，展开卡复用看板锚点。full ⇄ expanded ⇄ tucked 三形态任意互切。
- **多皮肤主题** —— 五套强调色与图表配色（Neutral / Sage / Azure / Crimson / Mauve）整体换肤；默认改为 Neutral（灰度）。按设备保留，不参与同步。

### 变更

- **用量趋势** —— 趋势图改为带数据点的多线图（不再是单线），每条指标各成一线。

### 修复

- **轻量模式** —— 整个贴边条现在可拖动（不再只是角落小把手），且按压仍能区分「点击展开」与「拖动移动」。

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

[Unreleased]: https://github.com/Buktal/VaultOne/compare/v1.3.0...HEAD
[1.3.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.3.0
[1.2.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.2.0
[1.1.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.1.0
[1.0.0]: https://github.com/Buktal/VaultOne/releases/tag/v1.0.0
