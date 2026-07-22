// Cloud-config conflict resolver (ADR-0005 / #6): when sync_config detects that
// local and remote both edited a shared config file, the UI shows this panel so
// the user picks which version wins per file (never last-write-wins).

import { useState } from "react"
import { toast } from "sonner"
import { useResolveConfigConflictMutation } from "@/app/store/api"
import { Button } from "@/components/ui/button"
import type {
  ConfigConflict,
  ConfigSyncChoice,
} from "@/types/generated/bindings"

export function ConflictResolver({
  conflicts,
}: {
  conflicts: ConfigConflict[]
}) {
  const [resolve, { isLoading }] = useResolveConfigConflictMutation()
  // Default every conflict to keep-local; the user flips to keep-remote per file.
  const [choices, setChoices] = useState<Record<string, ConfigSyncChoice>>(() =>
    Object.fromEntries(
      conflicts.map((c) => [c.path, "keep_local" as ConfigSyncChoice]),
    ),
  )

  const apply = async () => {
    const payload = conflicts.map((c) => ({
      file: c.file,
      choice: choices[c.path] ?? "keep_local",
    }))
    const r = await resolve(payload)
    if ("error" in r) {
      toast.error("冲突解决失败")
      return
    }
    toast.success(
      `已解决 ${conflicts.length} 个冲突${r.data?.pricing_changed ? "（定价已更新）" : ""}`,
    )
  }

  return (
    <div className="border-destructive/40 bg-destructive/5 flex flex-col gap-2 rounded-md border p-3">
      <p className="text-muted-foreground text-xs">
        本地和远端都改了下列配置文件，请逐个选择保留哪个版本。
      </p>
      {conflicts.map((c) => {
        const choice = choices[c.path] ?? "keep_local"
        return (
          <div key={c.path} className="flex flex-col gap-2 border-t pt-2">
            <code className="font-mono text-xs">{c.path}</code>
            <div className="grid grid-cols-2 gap-2">
              <PreviewColumn
                label="本地"
                active={choice === "keep_local"}
                content={c.local_preview}
                onSelect={() =>
                  setChoices((p) => ({ ...p, [c.path]: "keep_local" }))
                }
              />
              <PreviewColumn
                label="远端"
                active={choice === "keep_remote"}
                content={c.remote_preview}
                onSelect={() =>
                  setChoices((p) => ({ ...p, [c.path]: "keep_remote" }))
                }
              />
            </div>
          </div>
        )
      })}
      <Button size="sm" disabled={isLoading} onClick={apply}>
        {isLoading ? "应用中…" : "应用所选并同步"}
      </Button>
    </div>
  )
}

function PreviewColumn({
  label,
  active,
  content,
  onSelect,
}: {
  label: string
  active: boolean
  content: string
  onSelect: () => void
}) {
  return (
    <button
      type="button"
      onClick={onSelect}
      className={`flex flex-col gap-1 rounded-md border p-2 text-left transition-colors ${
        active ? "border-primary bg-primary/5" : "border-border"
      }`}
    >
      <span className="text-muted-foreground text-xs">
        {label}
        {active ? " ✓" : ""}
      </span>
      <pre className="max-h-32 overflow-auto whitespace-pre-wrap break-all font-mono text-[11px] text-muted-foreground">
        {content || "（空）"}
      </pre>
    </button>
  )
}
