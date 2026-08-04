// Pricing view (BLUEPRINT 成本定价): model pricing table with add /
// edit / delete (via Dialog), LiteLLM upstream fetch, pricing.json read/write,
// plus client-side search and single-column sort. Editing/删除 use icon buttons
// with tooltips; the toolbar is an icon group to keep density high. Table state
// (data, search/sort/pagination, delete) lives in usePricingTable; this file is
// just the render — dialog UI and the fetch/reload/save toolbar actions.

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
import { useState } from "react"
import { useTranslation } from "react-i18next"
import {
  useFetchLitellmMutation,
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
import type { PricingSortKey } from "@/features/pricing/derive"
import { usePricingTable } from "@/features/pricing/use-pricing-table"
import { useMutateWithToast } from "@/hooks/use-toast-mutation"
import { formatCost } from "@/lib/format"

import type { PricingEntry } from "@/types/generated/bindings"
import { EntryEditorDialog, emptyEntry } from "./entry-editor-dialog"

export function PricingView() {
  const { t } = useTranslation()
  const [fetchLitellm, { isLoading: fetching }] = useFetchLitellmMutation()
  const [reloadFile, { isLoading: reloading }] = useReloadPricingMutation()
  const [saveFile, { isLoading: savingFile }] = useSavePricingToFileMutation()
  const runWithToast = useMutateWithToast()

  const {
    isLoading,
    remove,
    search,
    setSearch,
    sortKey,
    sortDir,
    onSort,
    total,
    page,
    totalPages,
    paged,
    prevPage,
    nextPage,
    hasPrev,
    hasNext,
  } = usePricingTable()

  const [editing, setEditing] = useState<PricingEntry | null>(null)
  const [dialogOpen, setDialogOpen] = useState(false)

  function openNew() {
    setEditing(emptyEntry())
    setDialogOpen(true)
  }
  function openEdit(e: PricingEntry) {
    setEditing({ ...e })
    setDialogOpen(true)
  }

  async function onFetchLitellm() {
    await runWithToast(fetchLitellm, undefined, {
      success: {
        message: (count) => t("pricing.toast.fetched", { count: count ?? 0 }),
      },
      failed: { key: "pricing.toast.fetchFailed" },
    })
  }
  async function onReloadFile() {
    await runWithToast(reloadFile, undefined, {
      success: {
        message: (count) => t("pricing.toast.reloaded", { count: count ?? 0 }),
      },
      failed: { key: "pricing.toast.reloadFailed" },
    })
  }
  async function onSaveFile() {
    await runWithToast(saveFile, undefined, {
      success: { key: "pricing.toast.savedFile" },
      failed: { key: "pricing.toast.saveFileFailed" },
    })
  }

  const sortProps = { sortKey, sortDir, onSort }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-2">
        <div className="relative">
          <Search className="text-muted-foreground absolute top-1/2 left-2 size-3.5 -translate-y-1/2" />
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={t("pricing.searchPlaceholder")}
            className="h-8 w-44 pl-7"
            aria-label={t("pricing.searchAria")}
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
                  aria-label={t("pricing.fetchLitellm")}
                />
              }
            >
              <CloudDownload />
            </TooltipTrigger>
            <TooltipContent>{t("pricing.fetchLitellm")}</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="outline"
                  size="icon-sm"
                  disabled={reloading}
                  onClick={onReloadFile}
                  aria-label={t("pricing.reloadFile")}
                />
              }
            >
              <FileUp />
            </TooltipTrigger>
            <TooltipContent>{t("pricing.reloadFile")}</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="outline"
                  size="icon-sm"
                  disabled={savingFile}
                  onClick={onSaveFile}
                  aria-label={t("pricing.saveFile")}
                />
              }
            >
              <FileDown />
            </TooltipTrigger>
            <TooltipContent>{t("pricing.saveFile")}</TooltipContent>
          </Tooltip>
        </div>
        <div className="ml-auto" />
        <Button size="sm" onClick={openNew}>
          <Plus />
          {t("pricing.add")}
        </Button>
      </div>
      <Card>
        <CardHeader>
          <CardTitle>{t("pricing.title")}</CardTitle>
        </CardHeader>
        <CardContent>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>
                  <SortHeader
                    label={t("pricing.col.modelKey")}
                    k="model_key"
                    {...sortProps}
                  />
                </TableHead>
                <TableHead>
                  <SortHeader
                    label={t("pricing.col.displayName")}
                    k="display_name"
                    {...sortProps}
                  />
                </TableHead>
                <TableHead>
                  <SortHeader
                    label={t("usage.tokens.input")}
                    k="input_per_million"
                    {...sortProps}
                  />
                </TableHead>
                <TableHead>
                  <SortHeader
                    label={t("usage.tokens.output")}
                    k="output_per_million"
                    {...sortProps}
                  />
                </TableHead>
                <TableHead>
                  <SortHeader
                    label={t("usage.tokens.cacheRead")}
                    k="cache_read_per_million"
                    {...sortProps}
                  />
                </TableHead>
                <TableHead>
                  <SortHeader
                    label={t("usage.tokens.cacheCreation")}
                    k="cache_creation_per_million"
                    {...sortProps}
                  />
                </TableHead>
                <TableHead>{t("usage.logs.col.source")}</TableHead>
                <TableHead className="text-right">
                  {t("pricing.col.actions")}
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {isLoading ? (
                <TableRow>
                  <TableCell colSpan={8} className="text-muted-foreground">
                    {t("common.loading")}
                  </TableCell>
                </TableRow>
              ) : total === 0 ? (
                <TableRow>
                  <TableCell
                    colSpan={8}
                    className="text-muted-foreground py-8 text-center"
                  >
                    {t("pricing.noMatch")}
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
                      {formatCost(e.input_per_million)}
                    </TableCell>
                    <TableCell className="pr-4 text-right tabular-nums">
                      {formatCost(e.output_per_million)}
                    </TableCell>
                    <TableCell className="pr-4 text-right tabular-nums">
                      {formatCost(e.cache_read_per_million)}
                    </TableCell>
                    <TableCell className="pr-4 text-right tabular-nums">
                      {formatCost(e.cache_creation_per_million)}
                    </TableCell>
                    <TableCell>
                      <Badge variant={e.is_builtin ? "secondary" : "default"}>
                        {e.is_builtin
                          ? t("pricing.builtin")
                          : t("pricing.custom")}
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
                                aria-label={t("common.edit")}
                              />
                            }
                          >
                            <Pencil />
                          </TooltipTrigger>
                          <TooltipContent>{t("common.edit")}</TooltipContent>
                        </Tooltip>
                        <Tooltip>
                          <TooltipTrigger
                            render={
                              <Button
                                variant="ghost"
                                size="icon-sm"
                                onClick={() => remove(e.model_key)}
                                aria-label={t("common.delete")}
                              />
                            }
                          >
                            <Trash2 />
                          </TooltipTrigger>
                          <TooltipContent>{t("common.delete")}</TooltipContent>
                        </Tooltip>
                      </div>
                    </TableCell>
                  </TableRow>
                ))
              )}
            </TableBody>
          </Table>

          <div className="text-muted-foreground mt-3 flex items-center justify-between text-xs">
            <span>{t("usage.logs.pageInfo", { page, totalPages, total })}</span>
            <div className="flex gap-2">
              <Button
                variant="outline"
                size="sm"
                disabled={!hasPrev}
                onClick={prevPage}
              >
                {t("usage.logs.prevPage")}
              </Button>
              <Button
                variant="outline"
                size="sm"
                disabled={!hasNext}
                onClick={nextPage}
              >
                {t("usage.logs.nextPage")}
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
  k: PricingSortKey
  sortKey: PricingSortKey | null
  sortDir: "asc" | "desc"
  onSort: (k: PricingSortKey) => void
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
