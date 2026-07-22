// Pricing view (BLUEPRINT 成本定价; ADR-0006): model pricing table with add /
// edit / delete (via Dialog), LiteLLM upstream fetch, pricing.json read/write,
// plus client-side search and single-column sort. Editing/删除 use icon buttons
// with tooltips; the toolbar is an icon group to keep density high.

import {
  ChevronUp,
  CloudDownload,
  FileDown,
  FileUp,
  Pencil,
  Plus,
  Search,
  Trash2,
} from "lucide-react"
import { useMemo, useState } from "react"
import { toast } from "sonner"
import {
  useDeletePricingMutation,
  useFetchLitellmMutation,
  usePricingQuery,
  useReloadPricingMutation,
  useSavePricingToFileMutation,
} from "@/app/store/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"

import type { PricingEntry } from "@/types/generated/bindings"
import { EntryEditorDialog, emptyEntry } from "./entry-editor-dialog"

type SortKey = keyof PricingEntry

/** Client-side page size — the full list is already loaded; rendering all of
 * it at once jank-scrolls once it grows past a few hundred entries. */
const PAGE_SIZE = 50

export function PricingView() {
  const { data: entries = [], isLoading } = usePricingQuery()
  const [remove] = useDeletePricingMutation()
  const [fetchLitellm, { isLoading: fetching }] = useFetchLitellmMutation()
  const [reloadFile, { isLoading: reloading }] = useReloadPricingMutation()
  const [saveFile, { isLoading: savingFile }] = useSavePricingToFileMutation()

  const [search, setSearch] = useState("")
  const [sortKey, setSortKey] = useState<SortKey | null>(null)
  const [sortDir, setSortDir] = useState<"asc" | "desc">("asc")
  const [editing, setEditing] = useState<PricingEntry | null>(null)
  const [dialogOpen, setDialogOpen] = useState(false)
  const [offset, setOffset] = useState(0)

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    let list = q
      ? entries.filter(
          (e) =>
            e.model_key.toLowerCase().includes(q) ||
            e.display_name.toLowerCase().includes(q),
        )
      : entries
    if (sortKey) {
      list = [...list].sort((a, b) => {
        const av = a[sortKey] ?? 0
        const bv = b[sortKey] ?? 0
        const cmp =
          typeof av === "number" && typeof bv === "number"
            ? av - bv
            : String(av).localeCompare(String(bv))
        return sortDir === "asc" ? cmp : -cmp
      })
    }
    return list
  }, [entries, search, sortKey, sortDir])

  const total = filtered.length
  const totalPages = Math.max(1, Math.ceil(total / PAGE_SIZE))
  const page = Math.min(Math.floor(offset / PAGE_SIZE) + 1, totalPages)
  const paged = filtered.slice(offset, offset + PAGE_SIZE)

  function onSort(k: SortKey) {
    setOffset(0)
    if (sortKey === k) setSortDir((d) => (d === "asc" ? "desc" : "asc"))
    else {
      setSortKey(k)
      setSortDir("asc")
    }
  }

  function openNew() {
    setEditing(emptyEntry())
    setDialogOpen(true)
  }
  function openEdit(e: PricingEntry) {
    setEditing({ ...e })
    setDialogOpen(true)
  }

  async function onFetchLitellm() {
    const res = await fetchLitellm()
    if ("error" in res) toast.error("LiteLLM 拉取失败（离线时使用本地定价）")
    else toast.success(`已从 LiteLLM 合并 ${res.data ?? 0} 条定价`)
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

  const sortProps = { sortKey, sortDir, onSort }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-2">
        <div className="relative">
          <Search className="text-muted-foreground absolute top-1/2 left-2 size-3.5 -translate-y-1/2" />
          <Input
            value={search}
            onChange={(e) => {
              setSearch(e.target.value)
              setOffset(0)
            }}
            placeholder="搜索模型…"
            className="h-8 w-44 pl-7"
            aria-label="搜索模型"
          />
        </div>
        <div className="flex items-center gap-1">
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="outline"
                  size="icon-sm"
                  disabled={fetching}
                  onClick={onFetchLitellm}
                  aria-label="拉取 LiteLLM"
                />
              }
            >
              <CloudDownload />
            </TooltipTrigger>
            <TooltipContent>拉取 LiteLLM</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="outline"
                  size="icon-sm"
                  disabled={reloading}
                  onClick={onReloadFile}
                  aria-label="读取 pricing.json"
                />
              }
            >
              <FileUp />
            </TooltipTrigger>
            <TooltipContent>读取 pricing.json</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="outline"
                  size="icon-sm"
                  disabled={savingFile}
                  onClick={onSaveFile}
                  aria-label="写入 pricing.json"
                />
              }
            >
              <FileDown />
            </TooltipTrigger>
            <TooltipContent>写入 pricing.json</TooltipContent>
          </Tooltip>
        </div>
        <div className="ml-auto" />
        <Button size="sm" onClick={openNew}>
          <Plus />
          新增
        </Button>
      </div>
      <Card>
        <CardHeader>
          <CardTitle>成本定价（每百万 Token / USD）</CardTitle>
        </CardHeader>
        <CardContent>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>
                  <SortHeader label="模型标识" k="model_key" {...sortProps} />
                </TableHead>
                <TableHead>
                  <SortHeader
                    label="显示名称"
                    k="display_name"
                    {...sortProps}
                  />
                </TableHead>
                <TableHead>
                  <SortHeader
                    label="输入"
                    k="input_per_million"
                    {...sortProps}
                  />
                </TableHead>
                <TableHead>
                  <SortHeader
                    label="输出"
                    k="output_per_million"
                    {...sortProps}
                  />
                </TableHead>
                <TableHead>
                  <SortHeader
                    label="缓存命中"
                    k="cache_read_per_million"
                    {...sortProps}
                  />
                </TableHead>
                <TableHead>
                  <SortHeader
                    label="缓存创建"
                    k="cache_creation_per_million"
                    {...sortProps}
                  />
                </TableHead>
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
              ) : filtered.length === 0 ? (
                <TableRow>
                  <TableCell
                    colSpan={8}
                    className="text-muted-foreground py-8 text-center"
                  >
                    无匹配条目
                  </TableCell>
                </TableRow>
              ) : (
                paged.map((e) => (
                  <TableRow key={e.model_key}>
                    <TableCell className="font-mono text-xs">
                      {e.model_key}
                    </TableCell>
                    <TableCell>{e.display_name}</TableCell>
                    <TableCell className="pr-4 text-right tabular-nums">
                      {fmtRate(e.input_per_million)}
                    </TableCell>
                    <TableCell className="pr-4 text-right tabular-nums">
                      {fmtRate(e.output_per_million)}
                    </TableCell>
                    <TableCell className="pr-4 text-right tabular-nums">
                      {fmtRate(e.cache_read_per_million)}
                    </TableCell>
                    <TableCell className="pr-4 text-right tabular-nums">
                      {fmtRate(e.cache_creation_per_million)}
                    </TableCell>
                    <TableCell>
                      <Badge variant={e.is_builtin ? "secondary" : "default"}>
                        {e.is_builtin ? "内置" : "自定义"}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-right">
                      <div className="flex justify-end gap-1">
                        <Tooltip>
                          <TooltipTrigger
                            render={
                              <Button
                                variant="ghost"
                                size="icon-sm"
                                onClick={() => openEdit(e)}
                                aria-label="编辑"
                              />
                            }
                          >
                            <Pencil />
                          </TooltipTrigger>
                          <TooltipContent>编辑</TooltipContent>
                        </Tooltip>
                        <Tooltip>
                          <TooltipTrigger
                            render={
                              <Button
                                variant="ghost"
                                size="icon-sm"
                                onClick={() => remove(e.model_key)}
                                aria-label="删除"
                              />
                            }
                          >
                            <Trash2 />
                          </TooltipTrigger>
                          <TooltipContent>删除</TooltipContent>
                        </Tooltip>
                      </div>
                    </TableCell>
                  </TableRow>
                ))
              )}
            </TableBody>
          </Table>

          <div className="text-muted-foreground mt-3 flex items-center justify-between text-xs">
            <span>
              第 {page} / {totalPages} 页 · 共 {total} 条
            </span>
            <div className="flex gap-2">
              <Button
                variant="outline"
                size="sm"
                disabled={offset === 0}
                onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))}
              >
                上一页
              </Button>
              <Button
                variant="outline"
                size="sm"
                disabled={offset + PAGE_SIZE >= total}
                onClick={() => setOffset(offset + PAGE_SIZE)}
              >
                下一页
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>

      <EntryEditorDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        entry={editing}
        onSaved={() => {
          setDialogOpen(false)
          setEditing(null)
        }}
      />
    </div>
  )
}

function SortHeader({
  label,
  k,
  sortKey,
  sortDir,
  onSort,
}: {
  label: string
  k: SortKey
  sortKey: SortKey | null
  sortDir: "asc" | "desc"
  onSort: (k: SortKey) => void
}) {
  const active = sortKey === k
  return (
    <button
      type="button"
      onClick={() => onSort(k)}
      className={`inline-flex items-center gap-1 transition-colors hover:text-foreground ${
        active ? "text-foreground" : ""
      }`}
    >
      {label}
      <ChevronUp
        className={`size-3 transition-transform ${
          active && sortDir === "desc" ? "rotate-180" : ""
        } ${active ? "opacity-100" : "opacity-0"}`}
      />
    </button>
  )
}

/** Per-million rate → `$x.xxxx` display. */
function fmtRate(v: number | null | undefined): string {
  const n = Number(v ?? 0)
  return `$${n.toFixed(4)}`
}
