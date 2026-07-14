# 领域文档（Domain Docs）

工程技能在探索代码库时，应如何消费本仓库的领域文档。

## 探索前先读

- 仓库根目录的 **`CONTEXT.md`**，或
- 若仓库根目录存在 **`CONTEXT-MAP.md`**——它指向每个 context 各一份的 `CONTEXT.md`，阅读与当前主题相关的那些。
- **`docs/adr/`**——阅读涉及你即将改动区域的 ADR。多 context 仓库中，还要查看 `src/<context>/docs/adr/` 中 context 范围的决策。

如果上述文件尚不存在，**静默继续**。不要指出其缺失，也不要建议立即创建。`/domain-modeling` 技能（经 `/grill-with-docs` 与 `/improve-codebase-architecture` 触达）会在术语或决策真正落定时按需创建它们。

## 文件结构

VaultOne 是**单 context（single-context）**仓库：

```
/
├── CONTEXT.md
├── docs/adr/
│   ├── 0001-event-sourced-orders.md
│   └── 0002-postgres-for-write-model.md
└── src/
```

多 context 布局（根目录 `CONTEXT-MAP.md` 指向 `src/` 下各 context 的 `CONTEXT.md`）在此未使用。

## 使用术语表的词汇

当你的输出提到某个领域概念（出现在 issue 标题、重构提案、假设、测试名等处）时，使用 `CONTEXT.md` 中定义的术语。不要漂移到术语表明确避免的同义词。

如果你需要的概念还不在术语表中，这是一个信号——要么你在发明项目并不使用的语言（重新考虑），要么确实存在缺口（记下来交给 `/domain-modeling`）。

## 标注 ADR 冲突

如果你的输出与某个既有 ADR 相矛盾，请显式指出，而不是悄悄覆盖：

> _与 ADR-0007（event-sourced orders）冲突——但值得重新讨论，因为…_
