# Images

README 顶部的截图。按 UI 显示语言区分，每语言各一张（深色主题）。

```
docs/images/
├─ en/   English
├─ zh/   简体中文
└─ ja/   日本語
```

每个语言目录下 2 张：

| 文件 | 内容 |
| --- | --- |
| `dashboard.png` | 完整看板：统计卡（token 四桶 / 缓存命中率 / 请求数 / 成本）+ 趋势图 + 请求日志 |
| `lightweight.png` | 轻量速览模式：贴边半图标 + 悬停展开的今日用量 |

各语言 README 引用各自语言目录的图：

- `README.md` → `./docs/images/en/`
- `README.zh-CN.md` → `./docs/images/zh/`
- `README.ja-JP.md` → `./docs/images/ja/`

> 占位说明：`zh/` 与 `ja/` 当前暂用英文图占位。替换为各语言真实截图时，**保持同名覆盖**即可，README 引用路径无需改动。
