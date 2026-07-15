# VaultOne

[English](./README.md) | **简体中文**

> ⚠️ 早期开发中，尚不可用。

VaultOne 是一款桌面应用，它把你本地的 AI 助手会话日志转化为清晰的使用仪表盘——token 用量、成本、缓存效率、趋势——并通过 GitHub 仓库在你的各设备间同步数据。

- **只读采集**本地会话日志（目前支持 Claude Code，更多 provider 规划中）。
- **多设备同步**，以 GitHub 仓库作为轻量的纯文本后端。
- **本地仪表盘**，跨所有设备查询使用情况。

## 技术栈

- [Tauri](https://tauri.app/)（Rust 后端）+ [React 19](https://react.dev/) + [TypeScript](https://www.typescriptlang.org/)
- [Vite](https://vite.dev/) + [Tailwind CSS v4](https://tailwindcss.com/) + [shadcn/ui](https://ui.shadcn.com/)

## 前置条件

- [Node.js](https://nodejs.org/) LTS 与 [Yarn](https://yarnpkg.com/)
- [Rust](https://www.rust-lang.org/)（stable）——按你的系统参考 [Tauri 前置条件](https://tauri.app/start/prerequisites/)

## 快速开始

```bash
yarn install        # 安装前端依赖
yarn tauri dev      # 以开发模式运行桌面应用
```

## 脚本

| 命令 | 说明 |
| --- | --- |
| `yarn dev` | 仅运行 Web UI（Vite）。 |
| `yarn tauri dev` | 以开发模式运行完整的 Tauri 桌面应用。 |
| `yarn build` | 类型检查并构建 Web 产物。 |
| `yarn tauri build` | 构建发布版桌面二进制。 |
| `yarn lint` | 代码检查与格式校验（Biome）。 |
| `yarn format` | 自动格式化（Biome）。 |

## 许可证

[MIT](./LICENSE)
