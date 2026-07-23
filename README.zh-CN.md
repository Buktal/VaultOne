# VaultOne

> 本地优先的桌面看板，呈现你的 Claude Code token 用量与成本——直接读取 Claude Code 已写出的会话日志，并可选地通过你自己的 GitHub 仓库在多设备间同步。

[![Version](https://img.shields.io/github/v/release/Buktal/VaultOne?color=blue&label=version)](https://github.com/Buktal/VaultOne/releases)
[![平台](https://img.shields.io/badge/%E5%B9%B3%E5%8F%B0-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](https://github.com/Buktal/VaultOne/releases)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](./LICENSE)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)

[English](./README.md) | **简体中文** | [日本語](./README.ja-JP.md) | [更新日志](./CHANGELOG.zh-CN.md)

<img src="./docs/images/ad-zh.png" alt="VaultOne 看板">

---

## 为什么用 VaultOne？

Claude Code 每次运行都会在磁盘上写下会话日志。VaultOne 把这些日志转化为清晰的用量图景——**token、成本、缓存效率、趋势**——你无需架设代理、交出 API key，也无需把数据发送到任何地方。

整个产品由两点立场所塑造：

- **本地优先。** 看板在零网络环境下即可工作，读取你自己的日志就够了。
- **只读。** VaultOne 只*读取*会话日志，绝不修改，也绝不干预 Claude Code 的行为。Claude Code 照常运行，一如往常。

多设备同步存在，但它纯粹是一层 **opt-in** 的叠加能力，绝非使用本应用的前提。

## 亮点

- **读取你工具已产生的日志** —— 直接解析 Claude Code 留在磁盘上的会话日志。无需代理、无需 API key、无需联网。
- **贴合真实计费的 token 口径** —— 四桶消耗（input / output / cache creation / cache read）+ 缓存命中率 + 成本，在采集入库时捕获并冻结。
- **用你自己的 GitHub 仓库做多设备同步** —— 用量数据导出为纯文本，按设备与日期切分，写入你掌控的仓库；中间不经过任何第三方服务。
- **轻量速览模式** —— 缩成屏幕边缘的迷你条，常驻显示今日总数；或展开为复用看板的悬浮卡。full ⇄ expanded ⇄ tucked 三形态任意互切。
- **多皮肤主题** —— 五套强调色与图表配色（Neutral / Sage / Azure / Crimson / Mauve），整体换肤不动内容。
- **托盘常驻、后台采集** —— 增量扫描器在后台让看板保持新鲜，无需保留窗口。
- **自动更新 + 三语言** —— 直接从 GitHub Releases 安装签名更新；界面支持 English、简体中文、日本語。

## 截图

| | 浅色 | 深色 |
| --- | --- | --- |
| **看板** | <img src="./docs/images/light-usage.png" alt="看板（浅色）" width="320"> | <img src="./docs/images/dark-usage.png" alt="看板（深色）" width="320"> |
| **消耗** | <img src="./docs/images/light-consumption.png" alt="消耗（浅色）" width="320"> | <img src="./docs/images/dark-consumption.png" alt="消耗（深色）" width="320"> |
| **速览模式** | <img src="./docs/images/light-floating-card.png" alt="速览模式（浅色）" width="320"> | <img src="./docs/images/dark-floating-card.png" alt="速览模式（深色）" width="320"> |

## 下载

从 **[Releases](https://github.com/Buktal/VaultOne/releases)** 页面获取对应平台的安装包。

| 平台 | 安装包 |
| --- | --- |
| **Windows** | `.msi` 或 `.exe`（NSIS）安装程序 |
| **macOS** | `.dmg`（Apple Silicon / arm64） |
| **Linux** | `.deb`、`.AppImage`（部分版本提供 `.rpm`） |

**首次运行：** 启动 VaultOne——它会扫描本地的 Claude Code 会话日志，看板随即填充。无需账号、无需登录、无需联网。若要在多台机器间查看用量，在 **设置** 中开启同步，并指向一个你掌控的 GitHub 仓库。

> **macOS 提示：** 当前构建未签名。首次启动时请右键点击应用 → **打开**，或去除隔离属性：
> ```bash
> xattr -dr com.apple.quarantine /Applications/VaultOne.app
> ```

## 功能

### 看板

- **四桶 token 消耗** —— input、output、cache creation、cache read。
- **缓存命中率** —— `cache_read / (input + cache_creation + cache_read)`，与上游用量口径对齐。
- **请求数与成本** —— 总请求次数与总成本（USD），在采集入库时冻结。
- **用量趋势** —— 多线 token-成本图，每条指标一条线。
- **Per-call 请求日志** —— 模型、token 明细、成本、回合时长，以及 `stop_reason` / `service_tier` 语义标签。
- **Per-turn 视角** —— 整回合的成本与墙钟耗时，独立于单次调用计时。

### 采集

- **只读源日志** —— 解析 Claude Code 已写出的会话日志，绝不修改。
- **增量扫描** —— 基于游标的扫描器只处理变化部分。
- **托盘常驻后台调度器** —— 按定时器采集，无需保留窗口。
- **可插拔 provider** —— 当前 Claude Code，更多 provider 规划中。

### 同步（可选）

- **单机模式（Standalone）** —— 完整看板，零网络。
- **同步模式（Synced）** —— 通过你掌控的 GitHub 仓库在多设备间对齐用量。
- **纯文本产物** —— 按设备与日期切分（`data/<device>/usage-YYYY-MM-DD.jsonl`），diff 清晰可审。

### 成本与定价

- **可编辑的 per-model 定价** —— 覆盖种子价格，按你的数字计费。
- **回算（Rebill）** —— 对采集时缺价而记为零成本的记录补算，不重算已有历史。

### 交互

- **轻量速览模式** —— 贴边迷你条 + 可展开悬浮卡。
- **多皮肤主题** —— 五套配色，默认 Neutral（灰度）。
- **自动更新** —— 直接从 GitHub Releases 拉取签名安装包，设置页可手动检查。
- **浅色 / 深色主题、三语言、默认私密** —— 除非你开启同步，用量数据始终留在你的机器上。

## 工作原理

```
   Claude Code 会话日志
          │ （只读）
          ▼
       采集 ─────────▶ 本地库 ─────────▶ 看板
          │
          │ （可选 · 同步模式）
          ▼
   产物（纯文本，按设备 + 日期切分）
          │
    经由你的 GitHub 仓库 push / pull
          │
          ▼
      其他设备
```

一个 [Tauri 2](https://tauri.app/) 应用：Rust 后端负责采集、本地库与可选的 Git 仓库同步，React 前端通过生成的类型安全 IPC 绑定渲染看板。采集器是可插拔的 provider 模型（当前 Claude Code）；本地库是看板的唯一读取源；同步是把该库投影为纯文本、按设备与日期切分的产物的一层 opt-in 能力。

## 从源码构建

**前置条件：**[Node.js](https://nodejs.org/) LTS + [Yarn](https://yarnpkg.com/)，以及 [Rust](https://www.rust-lang.org/) stable（按你的系统参考 [Tauri 前置条件](https://tauri.app/start/prerequisites/)）。

```bash
yarn install     # 安装依赖
yarn dev         # 以开发模式运行桌面应用
yarn dist        # 构建发布版二进制
yarn check       # 静态检查（Biome + tsc + Rust fmt/clippy）——与 CI 同构
yarn test        # 运行测试套件
```

**技术栈：**[Tauri 2](https://tauri.app/)（Rust）· [React 19](https://react.dev/) · [TypeScript](https://www.typescriptlang.org/) · [Vite](https://vite.dev/) · [Tailwind CSS v4](https://tailwindcss.com/) · [shadcn/ui](https://ui.shadcn.com/) · [Redux Toolkit](https://redux-toolkit.js.org/) · [Recharts](https://recharts.org/)

## 参与贡献

欢迎提 issue 与建议。提交 PR 前请运行 `yarn check` 与 `yarn test`，确保本地通过 CI 门禁。较大的功能请先开 issue 讨论方案。

## 许可证

[MIT](./LICENSE) © VaultOne Contributors
