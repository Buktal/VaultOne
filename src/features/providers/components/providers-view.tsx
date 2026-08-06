// Providers view (供应商): the local provider list with drag-to-reorder,
// add / edit (via Sheet) / delete. Rows are a Card list (not a <table>) so the
// whole row can act as the dnd-kit drag handle, matching the group sidebar's
// reorder interaction. Empty state nudges toward creating a provider — the
// preset picker arrives on a later ticket, so the hint names both paths.

import { PointerActivationConstraints } from "@dnd-kit/dom"
import {
  DragDropProvider,
  type DragEndEvent,
  PointerSensor,
} from "@dnd-kit/react"
import { useSortable } from "@dnd-kit/react/sortable"
import { Pencil, Plus, Trash2 } from "lucide-react"
import { useState } from "react"
import { useTranslation } from "react-i18next"
import { useDeleteProviderMutation } from "@/app/store/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { providerEndpoint, providerModel } from "@/features/providers/derive"
import { useProvidersBrowser } from "@/features/providers/use-providers-browser"
import { useMutateWithToast } from "@/hooks/use-toast-mutation"
import { cn } from "@/lib/utils"

import type { Provider } from "@/types/generated/bindings"
import { ProviderFormSheet } from "./provider-form-sheet"

export function ProvidersView() {
  const { t } = useTranslation()
  const { providers, isLoading, onReorder } = useProvidersBrowser()
  const [remove] = useDeleteProviderMutation()
  const runWithToast = useMutateWithToast()

  const [sheetOpen, setSheetOpen] = useState(false)
  const [editing, setEditing] = useState<Provider | null>(null)

  // Whole-row drag handle: 6px of movement before a press becomes a drag —
  // clicks keep opening the edit sheet; moves reorder. Same constraints as the
  // group sidebar.
  const sensors = [
    PointerSensor.configure({
      activationConstraints: () => [
        new PointerActivationConstraints.Distance({ value: 6 }),
      ],
      preventActivation: () => false,
    }),
  ]

  function handleDragEnd(event: DragEndEvent): void {
    if (event.canceled) return
    const sourceId = event.operation.source?.id
    const targetId = event.operation.target?.id
    if (sourceId == null || targetId == null || sourceId === targetId) return
    onReorder(String(sourceId), String(targetId))
  }

  function openNew() {
    setEditing(null)
    setSheetOpen(true)
  }
  function openEdit(p: Provider) {
    setEditing(p)
    setSheetOpen(true)
  }
  async function onDelete(p: Provider) {
    await runWithToast(remove, p.id, {
      success: { key: "providers.toast.deleted", vars: { name: p.name } },
      failed: { key: "providers.toast.deleteFailed" },
    })
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4">
      <div className="flex items-center justify-end">
        <Button size="sm" onClick={openNew}>
          <Plus />
          {t("providers.add")}
        </Button>
      </div>
      <Card className="min-h-0 flex-1">
        <CardHeader>
          <CardTitle>{t("providers.title")}</CardTitle>
        </CardHeader>
        <CardContent className="flex min-h-0 flex-1 flex-col">
          {isLoading ? (
            <div className="text-muted-foreground py-12 text-center text-sm">
              {t("common.loading")}
            </div>
          ) : providers.length === 0 ? (
            <div className="text-muted-foreground py-12 text-center text-sm">
              {t("providers.empty")}
            </div>
          ) : (
            <div className="min-h-0 flex-1 -mr-2.5 overflow-auto pr-2.5">
              <div className="text-muted-foreground grid grid-cols-[minmax(10rem,1.2fr)_auto_1.4fr_1fr_auto] gap-3 px-4 pb-2 text-xs">
                <span>{t("providers.col.name")}</span>
                <span>{t("providers.col.category")}</span>
                <span>{t("providers.col.endpoint")}</span>
                <span>{t("providers.col.model")}</span>
                <span className="w-16" />
              </div>
              <DragDropProvider sensors={sensors} onDragEnd={handleDragEnd}>
                {providers.map((p, i) => (
                  <ProviderRow
                    key={p.id}
                    provider={p}
                    index={i}
                    onEdit={() => openEdit(p)}
                    onDelete={() => void onDelete(p)}
                  />
                ))}
              </DragDropProvider>
            </div>
          )}
        </CardContent>
      </Card>

      <ProviderFormSheet
        open={sheetOpen}
        onOpenChange={setSheetOpen}
        editing={editing}
        onSaved={() => setSheetOpen(false)}
      />
    </div>
  )
}

function ProviderRow({
  provider: p,
  index,
  onEdit,
  onDelete,
}: {
  provider: Provider
  index: number
  onEdit: () => void
  onDelete: () => void
}) {
  const { t } = useTranslation()
  const { ref, isDragging } = useSortable({ id: p.id, index })
  const endpoint = providerEndpoint(p)
  const model = providerModel(p)
  return (
    <div
      ref={ref}
      className={cn(
        "hover:bg-muted grid grid-cols-[minmax(10rem,1.2fr)_auto_1.4fr_1fr_auto] items-center gap-3 rounded-lg px-4 py-2 transition-colors",
        isDragging && "bg-muted opacity-70 shadow-sm",
      )}
    >
      <span className="truncate font-medium">{p.name}</span>
      <Badge variant="secondary">{t(`providers.category.${p.category}`)}</Badge>
      <span
        className="text-muted-foreground truncate font-mono text-xs"
        title={endpoint}
      >
        {endpoint || "—"}
      </span>
      <span className="text-muted-foreground truncate text-xs">
        {model || "—"}
      </span>
      <div className="flex justify-end gap-1">
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={onEdit}
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
                onClick={onDelete}
                aria-label={t("common.delete")}
              />
            }
          >
            <Trash2 />
          </TooltipTrigger>
          <TooltipContent>{t("common.delete")}</TooltipContent>
        </Tooltip>
      </div>
    </div>
  )
}
