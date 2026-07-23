// Cloud-config conflict resolver (ADR-0005 / #6): when sync_config detects that
// local and remote both edited a shared config file, the UI shows this panel so
// the user picks which version wins per file (never last-write-wins).

import { useState } from "react"
import { useTranslation } from "react-i18next"
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
  const { t } = useTranslation()
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
      toast.error(t("conflict.resolveFailed"))
      return
    }
    toast.success(
      t("conflict.resolved", { count: conflicts.length }) +
        (r.data?.pricing_changed ? t("conflict.resolvedPricing") : ""),
    )
  }

  return (
    <div className="border-destructive/40 bg-destructive/5 flex flex-col gap-2 rounded-md border p-3">
      <p className="text-muted-foreground text-xs">{t("conflict.intro")}</p>
      {conflicts.map((c) => {
        const choice = choices[c.path] ?? "keep_local"
        return (
          <div key={c.path} className="flex flex-col gap-2 border-t pt-2">
            <code className="font-mono text-xs">{c.path}</code>
            <div className="grid grid-cols-2 gap-2">
              <PreviewColumn
                label={t("conflict.local")}
                active={choice === "keep_local"}
                content={c.local_preview}
                onSelect={() =>
                  setChoices((p) => ({ ...p, [c.path]: "keep_local" }))
                }
              />
              <PreviewColumn
                label={t("conflict.remote")}
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
        {isLoading ? t("conflict.applying") : t("conflict.applyButton")}
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
  const { t } = useTranslation()
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
        {content || t("conflict.empty")}
      </pre>
    </button>
  )
}
