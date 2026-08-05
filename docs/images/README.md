# Images

README 用的截图，统一放在此目录。每个画面各一张浅色 + 一张深色（不再按 UI 语言区分，三个 README 共用同一套图）。

```
docs/images/
├─ ad-en.png / ad-zh.png / ad-ja.png            各语言 Hero 横幅（同一画面，可共用一张裁剪）
├─ light-usage.png    / dark-usage.png          看板（Dashboard）
├─ light-sessions.png / dark-sessions.png       会话浏览器（Sessions browser）★
├─ light-session-detail.png / dark-session-detail.png  会话详情（Session detail）★
├─ light-consumption.png / dark-consumption.png 消耗（Consumption）
└─ light-floating-card.png / dark-floating-card.png   轻量速览模式（Glance mode）
```

| 文件名 | 内容 |
| --- | --- |
| `*-usage.png` | 完整看板：统计卡（token 四桶 / 缓存命中率 / 请求数 / 成本）+ 趋势图 + 请求日志 + 侧边栏 |
| `*-sessions.png` | 会话浏览器（收藏 tab 或本地 tab 均可）：分组侧栏 + 会话表格 + 时间/来源/模型/设备筛选 |
| `*-session-detail.png` | 会话详情面板：右侧滑出的 transcript（按角色着色）+ 会话统计 + 收藏/分组操作 |
| `*-consumption.png` | 用量消耗视图 |
| `*-floating-card.png` | 轻量速览模式：贴边迷你条 + 可展开的悬浮卡 |

三个 README 都引用本目录的图：

- `README.md` / `README.zh-CN.md` / `README.ja-JP.md` → `./docs/images/`

> 带 ★ 的是 v1.6.0 README 新增引用的文件，尚未存在。替换截图时**保持同名覆盖**即可，README 引用路径无需改动；新增文件直接按上表文件名放入本目录。
