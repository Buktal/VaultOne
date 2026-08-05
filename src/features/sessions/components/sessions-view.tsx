// Sessions view — 会话管理入口. Two tabs (Local / Favorites), each with its
// own grouping track: Local tab groups by `local_group_id` (device-private),
// Favorites tab groups by `synced_group_id` (git-synced) and shows the source
// device per row. Clicking a row opens the detail Sheet with the transcript.
//
// Pure rendering only — all state, queries, mutations and the optimistic
// favorite / pending-group handling live in useSessionsBrowser (./use-sessions-
// browser). This component owns JSX, styles, i18n and the source-display helper
// (../source-labels). Mirrors library-view.tsx's split.

import dayjs from "dayjs"
import relativeTime from "dayjs/plugin/relativeTime"
import { CalendarRange, MessagesSquare, Search, Star } from "lucide-react"
import { useTranslation } from "react-i18next"
import { useDistinctModelsQuery } from "@/app/store/api"
import { effectiveDays, type Preset } from "@/app/store/slices/filterSlice"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { formatCost, formatInt, formatTokens } from "@/lib/format"
import { cn } from "@/lib/utils"
import type { SessionRow } from "@/types/generated/bindings"
import type { SessionTab } from "../derive"
import { sessionSourceLabel } from "../source-labels"
import { useSessionsBrowser } from "../use-sessions-browser"
import { GroupCreateDialog } from "./group-create-dialog"
import { GroupSidebar } from "./group-sidebar"
import { SessionDetailSheet } from "./session-detail-sheet"

dayjs.extend(relativeTime)

export function SessionsView() {
  const b = useSessionsBrowser()
  const { t } = useTranslation()
  // Narrow `preview` to a non-null local so the detail-sheet callbacks capture
  // a SessionRow, not SessionRow | null (TS will not narrow across callbacks
  // that read the field later).
  const preview = b.preview

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
      {/* Control row — tabs + filter chips on the left; collect · search on
        the right. Mirrors the request-log ControlBar's chip layout. */}
      <div className="flex flex-wrap items-center gap-2">
        <Tabs value={b.tab} onValueChange={(v) => b.setTab(v as SessionTab)}>
          <TabsList>
            <TabsTrigger value="local">{t("sessions.tab.local")}</TabsTrigger>
            <TabsTrigger value="favorites">
              {t("sessions.tab.favorites")}
            </TabsTrigger>
          </TabsList>
        </Tabs>
        <DateRangeChip
          preset={b.rangePreset}
          fromDay={b.fromDay}
          toDay={b.toDay}
          onPreset={b.setRangePreset}
          onFromDay={b.setFromDay}
          onToDay={b.setToDay}
        />
        <SourceSelect value={b.source} onChange={b.setSource} />
        <ModelSelect value={b.model} onChange={b.setModel} />
        {b.deviceOptions.length > 0 && b.tab === "favorites" ? (
          <DeviceSelect
            options={b.deviceOptions}
            value={b.deviceScope}
            onChange={b.setDeviceScope}
          />
        ) : null}
        <div className="ml-auto flex items-center gap-2">
          <div className="relative w-56">
            <Search className="text-muted-foreground absolute top-1/2 left-2 size-3.5 -translate-y-1/2" />
            <Input
              value={b.search}
              onChange={(e) => b.setSearch(e.target.value)}
              placeholder={t("sessions.searchPlaceholder")}
              className="h-8 pl-7"
              aria-label={t("sessions.searchPlaceholder")}
            />
          </div>
        </div>
      </div>

      {/* Sidebar + list */}
      <div className="flex min-h-0 flex-1 gap-3">
        <GroupSidebar
          trackGroups={b.trackGroups}
          grouped={b.grouped}
          totalCount={b.totalCount}
          selectedGroupId={b.selectedGroupId}
          onSelect={b.setSelectedGroupId}
          onCreate={b.openCreateGroup}
          onRename={b.renameGroup}
          onDelete={b.deleteGroup}
          pendingGroup={b.pendingGroup}
          busyGroupId={b.busyGroupId}
          track={b.effectiveTrack}
        />

        <Card className="flex min-h-0 flex-1 flex-col">
          <CardHeader>
            <CardTitle>{t("sessions.title")}</CardTitle>
          </CardHeader>
          <CardContent className="flex min-h-0 flex-1 flex-col">
            <QueryState
              isLoading={b.isLoading}
              error={b.error}
              isEmpty={!b.isLoading && b.visibleSessions.length === 0}
              emptyIcon={MessagesSquare}
              emptyLabel={
                b.search ? t("sessions.noMatch") : t("sessions.empty.title")
              }
              emptyDescription={b.search ? undefined : t("sessions.empty.desc")}
            >
              <SessionsTable
                rows={b.visibleSessions}
                effectiveFavorite={b.effectiveFavorite}
                onToggleFavorite={b.toggleFavorite}
                onOpen={b.setPreview}
                showDeviceColumn={b.showDeviceColumn}
                deviceLabel={b.deviceLabel}
              />
            </QueryState>
          </CardContent>
        </Card>
      </div>

      {preview ? (
        <SessionDetailSheet
          session={preview}
          favorited={b.effectiveFavorite(preview)}
          onClose={() => b.setPreview(null)}
          onToggleFavorite={() => b.toggleFavorite(preview)}
          editTitle={b.editTitle}
          titleDraft={b.titleDraft}
          onTitleDraft={b.setTitleDraft}
          onStartTitle={b.startEditTitle}
          onCancelTitle={b.cancelEditTitle}
          onCommitTitle={b.commitEditTitle}
          trackGroups={b.trackGroups}
          currentGroupId={
            b.effectiveTrack === "local"
              ? preview.local_group_id
              : preview.synced_group_id
          }
          onSetGroup={(groupId) => b.setSessionGroup(preview, groupId)}
          transcript={b.transcript}
          transcriptLoading={b.transcriptLoading}
          transcriptError={b.transcriptError}
          onRefreshTranscript={b.refetchTranscript}
          deviceLabel={(id) => b.deviceLabel.get(id) ?? id.slice(0, 8)}
        />
      ) : null}

      <GroupCreateDialog
        open={b.createGroupOpen}
        onClose={() => b.setCreateGroupOpen(false)}
        onCreate={b.createGroup}
        creating={b.pendingGroup !== null}
        track={b.effectiveTrack}
      />
    </div>
  )
}

function SessionsTable({
  rows,
  effectiveFavorite,
  onToggleFavorite,
  onOpen,
  showDeviceColumn,
  deviceLabel,
}: {
  rows: SessionRow[]
  effectiveFavorite: (s: SessionRow) => boolean
  onToggleFavorite: (s: SessionRow) => void
  onOpen: (s: SessionRow) => void
  showDeviceColumn: boolean
  deviceLabel: Map<string, string>
}) {
  const { t } = useTranslation()
  return (
    <div className="min-h-0 flex-1 -mr-2.5 overflow-auto pr-2.5">
      {/* table-fixed: column widths come from the header row, so the narrow
          numeric columns (w-20/w-24) are never stretched by extra horizontal
          space — the title column (no explicit width) absorbs the remainder. */}
      <Table className="table-fixed">
        <TableHeader>
          <TableRow>
            <TableHead className="w-10" />
            {/* The title column absorbs the remaining space (keeps the numeric
              columns at their fixed narrow widths when maximized) but caps at
              max-w so an ultra-wide window ellipsizes long titles instead of
              stretching the column indefinitely. */}
            <TableHead className="max-w-[24rem]">
              {t("sessions.col.title")}
            </TableHead>
            {showDeviceColumn ? (
              <TableHead className="w-28">{t("sessions.col.device")}</TableHead>
            ) : null}
            <TableHead className="w-48">{t("sessions.col.project")}</TableHead>
            <TableHead className="w-24">
              {t("sessions.col.lastActive")}
            </TableHead>
            <TableHead className="w-20 text-right">
              {t("sessions.col.requests")}
            </TableHead>
            <TableHead className="w-24 text-right">
              {t("sessions.col.tokens")}
            </TableHead>
            <TableHead className="w-20 text-right">
              {t("sessions.col.cost")}
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((s) => {
            const fav = effectiveFavorite(s)
            return (
              <TableRow key={`${s.device_id}/${s.id}`}>
                <TableCell>
                  <Tooltip>
                    <TooltipTrigger
                      render={
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          aria-label={
                            fav
                              ? t("sessions.row.unfavorite")
                              : t("sessions.row.favorite")
                          }
                          onClick={(e: React.MouseEvent) => {
                            e.stopPropagation()
                            onToggleFavorite(s)
                          }}
                        />
                      }
                    >
                      <Star
                        className={cn(
                          "size-4",
                          fav
                            ? "fill-accent-brand text-accent-brand"
                            : "text-muted-foreground",
                        )}
                      />
                    </TooltipTrigger>
                    <TooltipContent>
                      {fav
                        ? t("sessions.row.unfavorite")
                        : t("sessions.row.favorite")}
                    </TooltipContent>
                  </Tooltip>
                </TableCell>
                <TableCell>
                  {/* trackCursorAxis: the trigger is the full column width, so
                    a centered tooltip would float over the column's middle
                    (wrong for short titles) — anchor it to the cursor. */}
                  <Tooltip trackCursorAxis="both">
                    <TooltipTrigger
                      render={
                        <button
                          type="button"
                          className="hover:text-accent-brand-strong flex w-full min-w-0 flex-col items-start gap-0.5 text-left"
                          onClick={() => onOpen(s)}
                        />
                      }
                    >
                      <span className="block w-full min-w-0 truncate font-medium">
                        {s.title || t("sessions.untitled")}
                      </span>
                      <span className="text-muted-foreground text-xs">
                        {sessionSourceLabel(s.source)}
                      </span>
                    </TooltipTrigger>
                    <TooltipContent className="max-w-md">
                      {s.title || t("sessions.untitled")}
                    </TooltipContent>
                  </Tooltip>
                </TableCell>
                {showDeviceColumn ? (
                  <TableCell>
                    <Badge variant="outline" className="font-normal">
                      {deviceLabel.get(s.device_id) ?? s.device_id.slice(0, 8)}
                    </Badge>
                  </TableCell>
                ) : null}
                <TableCell className="text-muted-foreground text-xs">
                  <Tooltip trackCursorAxis="both">
                    <TooltipTrigger
                      render={<span className="block min-w-0 truncate" />}
                    >
                      {s.project_dir || "—"}
                    </TooltipTrigger>
                    <TooltipContent className="max-w-sm break-all">
                      {s.project_dir || "—"}
                    </TooltipContent>
                  </Tooltip>
                </TableCell>
                <TableCell className="text-muted-foreground text-xs">
                  <span
                    title={dayjs(s.last_active_at).format("YYYY-MM-DD HH:mm")}
                  >
                    {s.last_active_at ? dayjs(s.last_active_at).fromNow() : "—"}
                  </span>
                </TableCell>
                <TableCell className="text-right text-xs tabular-nums">
                  {formatInt(s.request_count)}
                </TableCell>
                <TableCell className="text-right text-xs tabular-nums">
                  {formatTokens(s.total_tokens)}
                </TableCell>
                <TableCell className="text-right text-xs tabular-nums">
                  {formatCost(s.total_cost_usd)}
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </Table>
    </div>
  )
}

/** "All sources" sentinel for the source dropdown. */
const ALL_SOURCES = "__all__"

/** "All devices" sentinel for the device dropdown. */
const ALL_DEVICES = "__all__"

/** Fixed source options — the providers sessions are collected from. Brand
 *  names are stable, so they live here rather than in i18n (mirrors the usage
 *  view's source-labels); only the "all" option and labels are localized. */
const SOURCE_OPTIONS: string[] = [
  "claude_code",
  "codex_cli",
  "gemini_cli",
  "grok_cli",
  "opencode",
]

function SourceSelect({
  value,
  onChange,
}: {
  value: string
  onChange: (v: string) => void
}) {
  const { t } = useTranslation()
  return (
    <Select
      value={value || ALL_SOURCES}
      onValueChange={(v) => onChange(v === ALL_SOURCES ? "" : (v ?? ""))}
    >
      <SelectTrigger
        className="h-8 w-40"
        aria-label={t("sessions.filter.source")}
      >
        <SelectValue className="min-w-0">
          {(val: string) =>
            val === ALL_SOURCES
              ? t("sessions.filter.allSources")
              : sessionSourceLabel(val)
          }
        </SelectValue>
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={ALL_SOURCES}>
          {t("sessions.filter.allSources")}
        </SelectItem>
        {SOURCE_OPTIONS.map((src) => (
          <SelectItem key={src} value={src}>
            {sessionSourceLabel(src)}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

/** "All models" sentinel for the model dropdown. */
const ALL_MODELS = "__all__"

/** Model dropdown — EXISTS semantics (a session that used the model at least
 *  once matches). Options come from the same distinct-models query the request
 *  log uses; the backend EXISTS filter narrows the session list itself. */
function ModelSelect({
  value,
  onChange,
}: {
  value: string
  onChange: (v: string) => void
}) {
  const { t } = useTranslation()
  const { data: models = [] } = useDistinctModelsQuery()
  return (
    <Select
      value={value || ALL_MODELS}
      onValueChange={(v) => onChange(v === ALL_MODELS ? "" : (v ?? ""))}
    >
      <SelectTrigger
        className="h-8 w-40"
        aria-label={t("sessions.filter.model")}
      >
        <SelectValue className="min-w-0">
          {(val: string) =>
            val === ALL_MODELS ? t("sessions.filter.allModels") : val
          }
        </SelectValue>
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={ALL_MODELS}>
          {t("sessions.filter.allModels")}
        </SelectItem>
        {models.map((m) => (
          <SelectItem key={m} value={m}>
            {m}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

/** Selectable presets in the popover — "custom" is implied by manual date entry. */
type SelectablePreset = Exclude<Preset, "custom">

const RANGE_PRESETS: Array<{ value: SelectablePreset; key: string }> = [
  { value: "today", key: "sessions.filter.today" },
  { value: "7d", key: "sessions.filter.last7d" },
  { value: "30d", key: "sessions.filter.last30d" },
  { value: "all", key: "sessions.filter.all" },
]

/** Time-range chip — a popover with preset buttons + custom date inputs.
 *  Mirrors the request-log ControlBar's DateRangeChip but reads/writes the
 *  sessions hook's local filter state (not Redux filterSlice). */
function DateRangeChip({
  preset,
  fromDay,
  toDay,
  onPreset,
  onFromDay,
  onToDay,
}: {
  preset: Preset
  fromDay: string
  toDay: string
  onPreset: (p: Preset) => void
  onFromDay: (d: string) => void
  onToDay: (d: string) => void
}) {
  const { t } = useTranslation()
  // The date inputs show the EFFECTIVE days — a dynamic preset (e.g. "today"
  // picked yesterday) renders the current day, not the frozen stored date.
  const { from_day: effFrom, to_day: effTo } = effectiveDays({
    range_preset: preset,
    from_day: fromDay,
    to_day: toDay,
  })
  const label =
    preset === "all"
      ? t("sessions.filter.allTime")
      : preset !== "custom"
        ? t(
            RANGE_PRESETS.find((p) => p.value === preset)?.key ??
              "sessions.filter.dateRange",
          )
        : fromDay || toDay
          ? fromDay === toDay
            ? fromDay || "…"
            : `${fromDay || "…"} → ${toDay || "…"}`
          : t("sessions.filter.allTime")

  return (
    <Popover>
      <PopoverTrigger
        render={
          <button
            type="button"
            className="border-border bg-card hover:bg-muted/60 flex h-8 max-w-full min-w-0 items-center gap-1.5 rounded-md border px-3 text-sm whitespace-nowrap"
          >
            <CalendarRange className="text-muted-foreground size-3.5 shrink-0" />
            <span className="min-w-0 truncate">{label}</span>
          </button>
        }
      />
      <PopoverContent align="start" className="w-72">
        <div className="bg-muted/60 inline-flex items-center gap-0.5 rounded-md p-0.5">
          {RANGE_PRESETS.map((p) => (
            <button
              key={p.value}
              type="button"
              onClick={() => onPreset(p.value)}
              className={cn(
                "focus-visible:ring-ring/40 rounded-[5px] px-2.5 py-1 text-xs font-medium transition-colors outline-none focus-visible:ring-2",
                preset === p.value
                  ? "bg-accent-tint text-accent-brand-strong"
                  : "text-muted-foreground hover:bg-muted/60 hover:text-foreground",
              )}
            >
              {t(p.key)}
            </button>
          ))}
        </div>
        <div className="mt-3 flex flex-col gap-2">
          <label className="flex flex-col gap-1 text-xs">
            <span className="text-muted-foreground">
              {t("sessions.filter.start")}
            </span>
            <input
              type="date"
              value={effFrom}
              onChange={(e) => onFromDay(e.target.value)}
              className="border-input bg-background h-8 rounded-md border px-2 text-xs"
            />
          </label>
          <label className="flex flex-col gap-1 text-xs">
            <span className="text-muted-foreground">
              {t("sessions.filter.end")}
            </span>
            <input
              type="date"
              value={effTo}
              onChange={(e) => onToDay(e.target.value)}
              className="border-input bg-background h-8 rounded-md border px-2 text-xs"
            />
          </label>
        </div>
      </PopoverContent>
    </Popover>
  )
}

/** Device dropdown for the Favorites tab — narrows "all devices" to one. */
function DeviceSelect({
  options,
  value,
  onChange,
}: {
  options: { id: string; label: string }[]
  value: string
  onChange: (v: string) => void
}) {
  const { t } = useTranslation()
  return (
    <Select
      value={value || ALL_DEVICES}
      onValueChange={(v) => onChange(v === ALL_DEVICES ? "" : (v ?? ""))}
    >
      <SelectTrigger
        className="h-8 w-40"
        aria-label={t("sessions.filter.device")}
      >
        <SelectValue className="min-w-0">
          {(val: string) =>
            val === ALL_DEVICES
              ? t("sessions.filter.allDevices")
              : (options.find((o) => o.id === val)?.label ?? val)
          }
        </SelectValue>
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={ALL_DEVICES}>
          {t("sessions.filter.allDevices")}
        </SelectItem>
        {options.map((o) => (
          <SelectItem key={o.id} value={o.id}>
            {o.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
