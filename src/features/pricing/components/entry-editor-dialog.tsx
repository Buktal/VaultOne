// Pricing entry editor as a modal Dialog (was an inline panel that pushed the
// table down). New and edit both flow through here; on save it calls the
// mutation and closes. User-edited entries are marked is_builtin=false so a
// later LiteLLM pull won't clobber them.

import { useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import { useSavePricingMutation } from "@/app/store/api"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"

import type { PricingEntry } from "@/types/generated/bindings"

export function emptyEntry(): PricingEntry {
  return {
    model_key: "",
    display_name: "",
    input_per_million: 0,
    output_per_million: 0,
    cache_read_per_million: 0,
    cache_creation_per_million: 0,
    is_builtin: false,
  }
}

export function EntryEditorDialog({
  open,
  onOpenChange,
  entry,
  onSaved,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  entry: PricingEntry | null
  onSaved: () => void
}) {
  const { t } = useTranslation()
  const [draft, setDraft] = useState<PricingEntry>(entry ?? emptyEntry())
  const [save, { isLoading: saving }] = useSavePricingMutation()

  useEffect(() => {
    if (open) setDraft(entry ?? emptyEntry())
  }, [entry, open])

  const set = (patch: Partial<PricingEntry>) =>
    setDraft((p) => ({ ...p, ...patch }))

  async function onSave() {
    if (!draft.model_key.trim()) {
      toast.error(t("pricing.toast.modelKeyRequired"))
      return
    }
    const res = await save({ entry: draft, isBuiltin: draft.is_builtin })
    if ("error" in res) toast.error(t("settings.toast.saveFailed"))
    else {
      toast.success(t("pricing.toast.saved", { key: draft.model_key }))
      onSaved()
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {entry?.model_key
              ? t("pricing.editor.editTitle")
              : t("pricing.editor.newTitle")}
          </DialogTitle>
          <DialogDescription>
            {t("pricing.editor.description")}
          </DialogDescription>
        </DialogHeader>

        <div className="grid grid-cols-2 gap-3">
          <Field label={t("pricing.col.modelKey")}>
            <Input
              value={draft.model_key}
              onChange={(e) => set({ model_key: e.target.value })}
            />
          </Field>
          <Field label={t("pricing.col.displayName")}>
            <Input
              value={draft.display_name}
              onChange={(e) => set({ display_name: e.target.value })}
            />
          </Field>
          <Field label={t("pricing.editor.input")}>
            <Input
              type="number"
              step="0.01"
              value={draft.input_per_million ?? 0}
              onChange={(e) =>
                set({ input_per_million: Number(e.target.value) })
              }
            />
          </Field>
          <Field label={t("pricing.editor.output")}>
            <Input
              type="number"
              step="0.01"
              value={draft.output_per_million ?? 0}
              onChange={(e) =>
                set({ output_per_million: Number(e.target.value) })
              }
            />
          </Field>
          <Field label={t("pricing.editor.cacheRead")}>
            <Input
              type="number"
              step="0.01"
              value={draft.cache_read_per_million ?? 0}
              onChange={(e) =>
                set({ cache_read_per_million: Number(e.target.value) })
              }
            />
          </Field>
          <Field label={t("pricing.editor.cacheCreation")}>
            <Input
              type="number"
              step="0.01"
              value={draft.cache_creation_per_million ?? 0}
              onChange={(e) =>
                set({ cache_creation_per_million: Number(e.target.value) })
              }
            />
          </Field>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("common.cancel")}
          </Button>
          <Button disabled={saving} onClick={onSave}>
            {saving ? t("common.saving") : t("common.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function Field({
  label,
  children,
}: {
  label: string
  children: React.ReactNode
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label className="text-muted-foreground text-xs">{label}</Label>
      {children}
    </div>
  )
}
