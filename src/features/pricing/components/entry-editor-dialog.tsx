// Pricing entry editor as a modal Dialog (was an inline panel that pushed the
// table down). New and edit both flow through here; on save it calls the
// mutation and closes. User-edited entries are marked is_builtin=false so a
// later LiteLLM pull won't clobber them.

import { useEffect, useState } from "react"
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
  const [draft, setDraft] = useState<PricingEntry>(entry ?? emptyEntry())
  const [save, { isLoading: saving }] = useSavePricingMutation()

  useEffect(() => {
    if (open) setDraft(entry ?? emptyEntry())
  }, [entry, open])

  const set = (patch: Partial<PricingEntry>) =>
    setDraft((p) => ({ ...p, ...patch }))

  async function onSave() {
    if (!draft.model_key.trim()) {
      toast.error("请填写模型标识 (model_key)")
      return
    }
    const res = await save({ entry: draft, isBuiltin: draft.is_builtin })
    if ("error" in res) toast.error("保存失败")
    else {
      toast.success(`已保存 ${draft.model_key}`)
      onSaved()
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {entry?.model_key ? "编辑模型定价" : "新增模型定价"}
          </DialogTitle>
          <DialogDescription>
            每百万 Token / USD。自定义条目（非内置）不会随 LiteLLM 拉取被覆盖。
          </DialogDescription>
        </DialogHeader>

        <div className="grid grid-cols-2 gap-3">
          <Field label="模型标识">
            <Input
              value={draft.model_key}
              onChange={(e) => set({ model_key: e.target.value })}
            />
          </Field>
          <Field label="显示名称">
            <Input
              value={draft.display_name}
              onChange={(e) => set({ display_name: e.target.value })}
            />
          </Field>
          <Field label="输入 / 1M">
            <Input
              type="number"
              step="0.01"
              value={draft.input_per_million ?? 0}
              onChange={(e) =>
                set({ input_per_million: Number(e.target.value) })
              }
            />
          </Field>
          <Field label="输出 / 1M">
            <Input
              type="number"
              step="0.01"
              value={draft.output_per_million ?? 0}
              onChange={(e) =>
                set({ output_per_million: Number(e.target.value) })
              }
            />
          </Field>
          <Field label="缓存命中 / 1M">
            <Input
              type="number"
              step="0.01"
              value={draft.cache_read_per_million ?? 0}
              onChange={(e) =>
                set({ cache_read_per_million: Number(e.target.value) })
              }
            />
          </Field>
          <Field label="缓存创建 / 1M">
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
            取消
          </Button>
          <Button disabled={saving} onClick={onSave}>
            {saving ? "保存中…" : "保存"}
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
