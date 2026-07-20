// Quick filters (BLUEPRINT 检索控制, inlined): time-range presets (segmented) +
// model / source selects + a button that opens the advanced FilterSheet. These
// are the high-frequency selectors; low-frequency (device scope, custom dates)
// live in the sheet so they don't crowd the cockpit.

import dayjs from "dayjs"
import { SlidersHorizontal } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  useQueryDistinctModelsQuery,
  useQueryDistinctSourcesQuery,
} from "@/features/usage/api"

import type { FilterState } from "./dashboard-view"

const ALL = "__all__"

type Preset = "today" | "7d" | "30d" | "all"

const PRESETS: Array<{ value: Preset; label: string }> = [
  { value: "today", label: "今天" },
  { value: "7d", label: "7天" },
  { value: "30d", label: "30天" },
  { value: "all", label: "全部" },
]

function dayStr(offset = 0) {
  return dayjs().add(offset, "day").format("YYYY-MM-DD")
}

/** Map the active preset from the current filter, or null for a custom range. */
export function derivePreset(f: FilterState): Preset | null {
  const today = dayStr()
  if (!f.from_day && !f.to_day) return "all"
  if (f.from_day === today && f.to_day === today) return "today"
  if (f.from_day === dayStr(-6) && f.to_day === today) return "7d"
  if (f.from_day === dayStr(-29) && f.to_day === today) return "30d"
  return null
}

function applyPreset(p: Preset): Pick<FilterState, "from_day" | "to_day"> {
  switch (p) {
    case "today":
      return { from_day: dayStr(), to_day: dayStr() }
    case "7d":
      return { from_day: dayStr(-6), to_day: dayStr() }
    case "30d":
      return { from_day: dayStr(-29), to_day: dayStr() }
    default:
      return { from_day: "", to_day: "" }
  }
}

export function QuickFilters({
  filter,
  onChange,
  onOpenAdvanced,
}: {
  filter: FilterState
  onChange: (next: FilterState) => void
  onOpenAdvanced: () => void
}) {
  const { data: sources = [] } = useQueryDistinctSourcesQuery()
  const { data: models = [] } = useQueryDistinctModelsQuery()
  const preset = derivePreset(filter)

  return (
    <div className="flex flex-wrap items-center gap-2">
      <div className="bg-muted/60 inline-flex items-center gap-0.5 rounded-md p-0.5">
        {PRESETS.map((p) => (
          <button
            key={p.value}
            type="button"
            onClick={() => onChange({ ...filter, ...applyPreset(p.value) })}
            className={`rounded-[5px] px-2.5 py-1 text-xs font-medium transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring/40 ${
              preset === p.value
                ? "bg-background text-foreground shadow-sm"
                : "text-muted-foreground hover:text-foreground"
            }`}
          >
            {p.label}
          </button>
        ))}
      </div>

      <Select
        value={filter.model || ALL}
        onValueChange={(v) =>
          onChange({ ...filter, model: v && v !== ALL ? v : "" })
        }
      >
        <SelectTrigger className="h-8 w-36" aria-label="模型">
          <SelectValue placeholder="全部模型" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={ALL}>全部模型</SelectItem>
          {models.map((m) => (
            <SelectItem key={m} value={m}>
              {m}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Select
        value={filter.source || ALL}
        onValueChange={(v) =>
          onChange({ ...filter, source: v && v !== ALL ? v : "" })
        }
      >
        <SelectTrigger className="h-8 w-36" aria-label="来源">
          <SelectValue placeholder="全部来源" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={ALL}>全部来源</SelectItem>
          {sources.map((s) => (
            <SelectItem key={s} value={s}>
              {s}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Button
        variant="outline"
        size="icon-sm"
        onClick={onOpenAdvanced}
        aria-label="高级筛选"
      >
        <SlidersHorizontal />
      </Button>
    </div>
  )
}
