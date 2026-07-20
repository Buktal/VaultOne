// Filter summary — removable chips for every non-default filter field. Hidden
// entirely when no constraint is active (gap doesn't reserve a row).

import { X } from "lucide-react"

import { Badge } from "@/components/ui/badge"

import type { FilterState } from "./dashboard-view"

export function FilterSummary({
  filter,
  onClear,
}: {
  filter: FilterState
  onClear: (key: keyof FilterState) => void
}) {
  const chips: Array<{ key: keyof FilterState; label: string }> = []
  if (filter.from_day)
    chips.push({ key: "from_day", label: `起 ${filter.from_day}` })
  if (filter.to_day) chips.push({ key: "to_day", label: `止 ${filter.to_day}` })
  if (filter.model) chips.push({ key: "model", label: `模型 ${filter.model}` })
  if (filter.source)
    chips.push({ key: "source", label: `来源 ${filter.source}` })
  if (filter.device_scope)
    chips.push({ key: "device_scope", label: `设备 ${filter.device_scope}` })

  if (chips.length === 0) return null

  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {chips.map((c) => (
        <Badge key={c.key} variant="outline" className="gap-1 py-0.5">
          <span className="text-muted-foreground">{c.label}</span>
          <button
            type="button"
            onClick={() => onClear(c.key)}
            aria-label={`清除 ${c.label}`}
            className="text-muted-foreground transition-colors hover:text-foreground"
          >
            <X className="size-3" />
          </button>
        </Badge>
      ))}
    </div>
  )
}
