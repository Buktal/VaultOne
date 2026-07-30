// Provider tag → 来源可读名映射 (来源筛选下拉用).
//
// `source` 是每条用量记录上的稳定 provider tag (即 RawUsage.source, 如
// "claude_code" / "codex_cli"). 筛选下拉显示可读名而非 snake_case 原值,
// 未知 tag 原样回退, 保证未来新增 provider 在补映射前也能正常显示.
//
// 这里只给"平台名". request-log-table.tsx 的 providerLabel() 自带 "(Session)"
// 后缀 —— 那是日志页语义 (一行 ≈ 一个 session), 不在此共享.

const SOURCE_LABELS: Record<string, string> = {
  claude_code: "Claude Code",
  codex_cli: "Codex",
  gemini_cli: "Gemini CLI",
  opencode: "OpenCode",
  grok_cli: "Grok",
}

/** 把 provider tag 转成展示名, 未知 tag 原样返回. */
export function sourceLabel(tag: string): string {
  return SOURCE_LABELS[tag] ?? tag
}
