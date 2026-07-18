// Filter bar: source / model / time-range / device selectors + manual collect
// (BLUEPRINT 检索控制). Sources & models come from the Local Store.

import { Activity, RefreshCw } from "lucide-react"
import { toast } from "sonner"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { useListDevicesQuery } from "@/features/settings/api"
import {
  useCollectNowMutation,
  useQueryDistinctModelsQuery,
  useQueryDistinctSourcesQuery,
} from "@/features/usage/api"

import type { FilterState } from "./dashboard-view"

const ALL = "__all__"

export function FilterBar({
  filter,
  onChange,
}: {
  filter: FilterState
  onChange: (next: FilterState) => void
}) {
  const { data: sources = [] } = useQueryDistinctSourcesQuery()
  const { data: models = [] } = useQueryDistinctModelsQuery()
  const { data: devices = [] } = useListDevicesQuery()
  const [collect, { isLoading: collecting }] = useCollectNowMutation()

  async function onCollect() {
    const res = await collect()
    if ("error" in res) {
      toast.error("采集失败")
      return
    }
    const r = res.data
    toast.success(
      `采集完成：新增 ${r?.rows_inserted ?? 0} 条（扫描 ${r?.files_scanned ?? 0} 文件）`,
    )
  }

  return (
    <div className="bg-card flex flex-wrap items-end gap-4 rounded-lg border p-4">
      <FilterField label="来源">
        <Select
          value={filter.source || ALL}
          onValueChange={(v) =>
            onChange({ ...filter, source: v && v !== ALL ? v : "" })
          }
        >
          <SelectTrigger className="w-40">
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
      </FilterField>

      <FilterField label="模型">
        <Select
          value={filter.model || ALL}
          onValueChange={(v) =>
            onChange({ ...filter, model: v && v !== ALL ? v : "" })
          }
        >
          <SelectTrigger className="w-48">
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
      </FilterField>

      <FilterField label="设备">
        <Select
          value={filter.device_scope || ALL}
          onValueChange={(v) =>
            onChange({ ...filter, device_scope: v && v !== ALL ? v : "" })
          }
        >
          <SelectTrigger className="w-44">
            <SelectValue placeholder="全部设备" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ALL}>全部设备</SelectItem>
            {devices.map((d) => (
              <SelectItem key={d.device_id} value={d.device_id}>
                {d.display_name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </FilterField>

      <FilterField label="起始日期">
        <Input
          type="date"
          className="w-40"
          value={filter.from_day}
          onChange={(e) => onChange({ ...filter, from_day: e.target.value })}
        />
      </FilterField>

      <FilterField label="结束日期">
        <Input
          type="date"
          className="w-40"
          value={filter.to_day}
          onChange={(e) => onChange({ ...filter, to_day: e.target.value })}
        />
      </FilterField>

      <div className="ml-auto flex items-center gap-2">
        <Button
          variant="outline"
          size="sm"
          disabled={collecting}
          onClick={() => onChange({ ...filter })}
        >
          <RefreshCw className="size-4" />
          刷新
        </Button>
        <Button size="sm" disabled={collecting} onClick={onCollect}>
          <Activity className="size-4" />
          {collecting ? "采集中…" : "采集本地日志"}
        </Button>
      </div>
    </div>
  )
}

function FilterField({
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
