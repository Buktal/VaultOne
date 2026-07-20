// Advanced filter sheet (BLUEPRINT 检索控制, low-frequency): device scope +
// custom from/to dates. Slides in from the right; the inlined QuickFilters hold
// the high-frequency presets + model + source.

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
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet"
import { useListDevicesQuery } from "@/features/settings/api"

import type { FilterState } from "./dashboard-view"

const ALL = "__all__"

export function FilterSheet({
  open,
  onOpenChange,
  filter,
  onChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  filter: FilterState
  onChange: (next: FilterState) => void
}) {
  const { data: devices = [] } = useListDevicesQuery()

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent>
        <SheetHeader>
          <SheetTitle>高级筛选</SheetTitle>
          <SheetDescription>
            设备范围与自定义日期。时间预设可从看板顶部快速切换。
          </SheetDescription>
        </SheetHeader>

        <div className="flex flex-col gap-4">
          <Field label="设备范围">
            <Select
              value={filter.device_scope || ALL}
              onValueChange={(v) =>
                onChange({
                  ...filter,
                  device_scope: v && v !== ALL ? v : "",
                })
              }
            >
              <SelectTrigger className="w-full">
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
          </Field>

          <div className="grid grid-cols-2 gap-3">
            <Field label="起始日期">
              <Input
                type="date"
                value={filter.from_day}
                onChange={(e) =>
                  onChange({ ...filter, from_day: e.target.value })
                }
              />
            </Field>
            <Field label="结束日期">
              <Input
                type="date"
                value={filter.to_day}
                onChange={(e) =>
                  onChange({ ...filter, to_day: e.target.value })
                }
              />
            </Field>
          </div>
        </div>

        <SheetFooter>
          <Button
            variant="outline"
            onClick={() =>
              onChange({
                ...filter,
                from_day: "",
                to_day: "",
                device_scope: "",
              })
            }
          >
            重置日期与设备
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
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
