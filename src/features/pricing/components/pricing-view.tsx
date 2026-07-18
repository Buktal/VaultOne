// Pricing view (BLUEPRINT 成本定价; ADR-0006): model pricing table with add /
// edit / delete, LiteLLM upstream fetch, and local pricing.json read/write.

import {
  CloudDownload,
  FileDown,
  FileUp,
  Pencil,
  Plus,
  Trash2,
} from "lucide-react"
import { useState } from "react"
import { toast } from "sonner"

import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import {
  useDeletePricingEntryMutation,
  useFetchLitellmPricingMutation,
  useListPricingQuery,
  useReloadPricingFromFileMutation,
  useSavePricingEntryMutation,
  useSavePricingToFileMutation,
} from "@/features/pricing/api"

import type { PricingEntry } from "@/types/generated/bindings"

function emptyEntry(): PricingEntry {
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

export function PricingView() {
  const { data: entries = [], isLoading } = useListPricingQuery()
  const [save, { isLoading: saving }] = useSavePricingEntryMutation()
  const [remove] = useDeletePricingEntryMutation()
  const [fetchLitellm, { isLoading: fetching }] =
    useFetchLitellmPricingMutation()
  const [reloadFile, { isLoading: reloading }] =
    useReloadPricingFromFileMutation()
  const [saveFile, { isLoading: savingFile }] = useSavePricingToFileMutation()

  const [draft, setDraft] = useState<PricingEntry | null>(null)

  async function onSave() {
    if (!draft) return
    if (!draft.model_key.trim()) {
      toast.error("请填写模型标识 (model_key)")
      return
    }
    const res = await save({ entry: draft, is_builtin: draft.is_builtin })
    if ("error" in res) toast.error("保存失败")
    else toast.success(`已保存 ${draft.model_key}`)
    setDraft(null)
  }

  async function onFetchLitellm() {
    const res = await fetchLitellm()
    if ("error" in res) {
      toast.error("LiteLLM 拉取失败（离线时使用本地定价）")
      return
    }
    toast.success(`已从 LiteLLM 合并 ${res.data ?? 0} 条定价`)
  }

  async function onReloadFile() {
    const res = await reloadFile()
    if ("error" in res) toast.error("读取 pricing.json 失败")
    else toast.success(`从 pricing.json 载入 ${res.data ?? 0} 条`)
  }

  async function onSaveFile() {
    const res = await saveFile()
    if ("error" in res) toast.error("写入 pricing.json 失败")
    else toast.success("已写入 pricing.json")
  }

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader className="flex-row items-center justify-between">
          <CardTitle>成本定价（每百万 Token / USD）</CardTitle>
          <div className="flex flex-wrap gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={fetching}
              onClick={onFetchLitellm}
            >
              <CloudDownload className="size-4" />
              {fetching ? "拉取中…" : "拉取 LiteLLM"}
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={reloading}
              onClick={onReloadFile}
            >
              <FileUp className="size-4" />
              读取 pricing.json
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={savingFile}
              onClick={onSaveFile}
            >
              <FileDown className="size-4" />
              写入 pricing.json
            </Button>
          </div>
        </CardHeader>
        <CardContent>
          <div className="mb-3 flex justify-end">
            <Button size="sm" onClick={() => setDraft(emptyEntry())}>
              <Plus className="size-4" />
              新增模型定价
            </Button>
          </div>

          {draft ? (
            <EntryEditor
              draft={draft}
              onChange={setDraft}
              onSave={onSave}
              onCancel={() => setDraft(null)}
              saving={saving}
            />
          ) : null}

          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>模型标识</TableHead>
                <TableHead>显示名称</TableHead>
                <TableHead className="text-right">输入</TableHead>
                <TableHead className="text-right">输出</TableHead>
                <TableHead className="text-right">缓存命中</TableHead>
                <TableHead className="text-right">缓存创建</TableHead>
                <TableHead>来源</TableHead>
                <TableHead className="text-right">操作</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {isLoading ? (
                <TableRow>
                  <TableCell colSpan={8} className="text-muted-foreground">
                    加载中…
                  </TableCell>
                </TableRow>
              ) : (
                entries.map((e) => (
                  <TableRow key={e.model_key}>
                    <TableCell className="font-mono text-xs">
                      {e.model_key}
                    </TableCell>
                    <TableCell>{e.display_name}</TableCell>
                    <TableCell className="text-right tabular-nums">
                      {fmtRate(e.input_per_million)}
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      {fmtRate(e.output_per_million)}
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      {fmtRate(e.cache_read_per_million)}
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      {fmtRate(e.cache_creation_per_million)}
                    </TableCell>
                    <TableCell className="text-muted-foreground text-xs">
                      {e.is_builtin ? "内置" : "自定义"}
                    </TableCell>
                    <TableCell className="text-right">
                      <div className="flex justify-end gap-1">
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => setDraft({ ...e })}
                        >
                          <Pencil className="size-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => remove(e.model_key)}
                        >
                          <Trash2 className="size-4" />
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))
              )}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </div>
  )
}

function EntryEditor({
  draft,
  onChange,
  onSave,
  onCancel,
  saving,
}: {
  draft: PricingEntry
  onChange: (next: PricingEntry) => void
  onSave: () => void
  onCancel: () => void
  saving: boolean
}) {
  const set = (patch: Partial<PricingEntry>) => onChange({ ...draft, ...patch })
  return (
    <div className="bg-muted/40 mb-3 grid grid-cols-2 gap-3 rounded-md border p-3 md:grid-cols-4">
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
          onChange={(e) => set({ input_per_million: Number(e.target.value) })}
        />
      </Field>
      <Field label="输出 / 1M">
        <Input
          type="number"
          step="0.01"
          value={draft.output_per_million ?? 0}
          onChange={(e) => set({ output_per_million: Number(e.target.value) })}
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
      <div className="col-span-2 flex items-end justify-end gap-2">
        <Button variant="outline" size="sm" onClick={onCancel}>
          取消
        </Button>
        <Button size="sm" disabled={saving} onClick={onSave}>
          保存
        </Button>
      </div>
    </div>
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

/** Per-million rate → `$x.xxxx` display. */
function fmtRate(v: number | null | undefined): string {
  const n = Number(v ?? 0)
  return `$${n.toFixed(4)}`
}
