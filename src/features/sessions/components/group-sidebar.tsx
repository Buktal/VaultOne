// Group sidebar — the per-track group switcher for the sessions list. Renders
// "All", one row per group (with a live session count + a popover for rename /
// delete), "Ungrouped", an optimistic pending row for an in-flight synced-group
// create, and a "+ New group" button. Local groups rename/delete immediately;
// synced groups round-trip through git so their row shows a spinner while busy.
//
// Pure rendering — selection state, CRUD handlers, and the pending/busy flags
// come from useSessionsBrowser. The rename input inside each row's popover is
// transient local state (the hook only learns the new name on submit).

import {
  DndContext,
  type DragEndEvent,
  PointerSensor,
  useSensor,
  useSensors,
} from "@dnd-kit/core"
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable"
import { CSS } from "@dnd-kit/utilities"
import {
  Check,
  FolderTree,
  Loader2,
  MoreHorizontal,
  Pencil,
  Plus,
  Trash2,
  X,
} from "lucide-react"
import { useState } from "react"
import { useTranslation } from "react-i18next"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import { ScrollArea } from "@/components/ui/scroll-area"
import { cn } from "@/lib/utils"
import type { SessionGroup } from "@/types/generated/bindings"
import {
  ALL_GROUPS,
  type GroupedSessions,
  type GroupTrack,
  reorderGroupIds,
  UNGROUPED,
} from "../derive"

export function GroupSidebar({
  trackGroups,
  grouped,
  totalCount,
  selectedGroupId,
  onSelect,
  onCreate,
  onRename,
  onDelete,
  onReorder,
  pendingGroup,
  busyGroupId,
  track,
}: {
  trackGroups: SessionGroup[]
  grouped: GroupedSessions
  totalCount: number
  selectedGroupId: string
  onSelect: (id: string) => void
  onCreate: () => void
  onRename: (g: SessionGroup, name: string) => Promise<void>
  onDelete: (g: SessionGroup) => Promise<void>
  onReorder: (orderedIds: string[]) => void
  pendingGroup: string | null
  busyGroupId: string | null
  track: GroupTrack
}) {
  const { t } = useTranslation()
  const countById = new Map(
    grouped.groups.map((x) => [x.group.id, x.sessions.length]),
  )
  const ungroupedCount = grouped.ungrouped.length
  const sensors = useSensors(
    useSensor(PointerSensor, {
      // 6px of movement before a press becomes a drag — clicks keep selecting
      // the row / opening its popover; moves reorder.
      activationConstraint: { distance: 6 },
    }),
  )

  function handleDragEnd(event: DragEndEvent): void {
    const { active, over } = event
    if (!over || active.id === over.id) return
    const next = reorderGroupIds(
      trackGroups.map((g) => g.id),
      String(active.id),
      String(over.id),
    )
    if (next) onReorder(next)
  }

  return (
    <div className="border-border bg-card flex min-h-0 w-52 shrink-0 flex-col gap-1 rounded-lg border p-2">
      <div className="text-muted-foreground px-2 py-1 text-xs font-medium">
        {track === "local"
          ? t("sessions.group.localTitle")
          : t("sessions.group.syncedTitle")}
      </div>
      {/* min-h-0: without it the ScrollArea grows with its content, pushing
        the "+ New group" button (below the scroll region) out of the viewport
        once the group list gets long. Mirrors the right Card's
        `flex min-h-0 flex-1` pattern. */}
      <ScrollArea className="min-h-0 flex-1">
        <div className="flex flex-col gap-0.5 pr-1">
          <SidebarItem
            label={t("sessions.group.all")}
            count={totalCount}
            active={selectedGroupId === ALL_GROUPS}
            onClick={() => onSelect(ALL_GROUPS)}
          />
          {/* Only the custom group rows are sortable — the ALL / UNGROUPED
            sentinels stay outside DndContext so they can never move. */}
          <DndContext sensors={sensors} onDragEnd={handleDragEnd}>
            <SortableContext
              items={trackGroups.map((g) => g.id)}
              strategy={verticalListSortingStrategy}
            >
              {trackGroups.map((g) => (
                <GroupRow
                  key={g.id}
                  group={g}
                  count={countById.get(g.id) ?? 0}
                  active={selectedGroupId === g.id}
                  onSelect={() => onSelect(g.id)}
                  onRename={onRename}
                  onDelete={onDelete}
                  busy={busyGroupId === g.id}
                />
              ))}
            </SortableContext>
          </DndContext>
          {pendingGroup ? (
            <div className="text-muted-foreground flex items-center gap-2 rounded-md px-2 py-1.5 text-sm">
              <Loader2 className="size-3.5 animate-spin" />
              <span className="truncate">{pendingGroup}</span>
            </div>
          ) : null}
          <SidebarItem
            label={t("sessions.group.ungrouped")}
            count={ungroupedCount}
            active={selectedGroupId === UNGROUPED}
            onClick={() => onSelect(UNGROUPED)}
          />
        </div>
      </ScrollArea>
      <Button
        variant="outline"
        size="sm"
        className="mt-1 justify-start"
        onClick={onCreate}
        disabled={pendingGroup !== null}
      >
        <Plus />
        {t("sessions.group.create")}
      </Button>
    </div>
  )
}

function SidebarItem({
  label,
  count,
  active,
  onClick,
}: {
  label: string
  count: number
  active: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "hover:bg-muted flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors",
        active && "bg-muted text-foreground",
        !active && "text-muted-foreground",
      )}
    >
      <FolderTree className="size-3.5 shrink-0" />
      <span className="flex-1 truncate">{label}</span>
      <span className="text-muted-foreground/70 text-xs tabular-nums">
        {count}
      </span>
    </button>
  )
}

function GroupRow({
  group: g,
  count,
  active,
  onSelect,
  onRename,
  onDelete,
  busy,
}: {
  group: SessionGroup
  count: number
  active: boolean
  onSelect: () => void
  onRename: (g: SessionGroup, name: string) => Promise<void>
  onDelete: (g: SessionGroup) => Promise<void>
  busy: boolean
}) {
  const { t } = useTranslation()
  const [renaming, setRenaming] = useState(false)
  const [draft, setDraft] = useState(g.name)
  const [popoverOpen, setPopoverOpen] = useState(false)
  // The whole row is the drag handle (no separate grip icon — the sidebar is
  // 208px wide). Busy rows are disabled: a rename/delete in flight can't be
  // reordered out from under.
  const { setNodeRef, transform, transition, isDragging, listeners } =
    useSortable({ id: g.id, disabled: busy })

  function startRename() {
    setDraft(g.name)
    setRenaming(true)
  }

  async function commitRename() {
    const name = draft.trim()
    setRenaming(false)
    setPopoverOpen(false)
    if (name && name !== g.name) {
      await onRename(g, name)
    }
  }

  async function confirmDelete() {
    setPopoverOpen(false)
    await onDelete(g)
  }

  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition }}
      {...listeners}
      className={cn(
        "group/grow hover:bg-muted flex items-center gap-1 rounded-md px-2 py-1.5 text-sm transition-colors",
        active ? "bg-muted text-foreground" : "text-muted-foreground",
        busy && "opacity-60",
        // The dragged row floats above its siblings while the others make
        // room via the sortable transform.
        isDragging && "relative z-10 opacity-60 shadow-sm",
      )}
    >
      <button
        type="button"
        className="flex min-w-0 flex-1 items-center gap-2 text-left"
        onClick={onSelect}
        disabled={busy}
      >
        {busy ? (
          <Loader2 className="size-3.5 shrink-0 animate-spin" />
        ) : (
          <FolderTree className="size-3.5 shrink-0" />
        )}
        <span className="flex-1 truncate">{g.name}</span>
      </button>
      {/* The count always sits flush right, matching the plain rows. The
        action slot next to it starts at zero width and stays clipped, so the
        ⋮ occupies no space at rest; on hover the slot expands and the count
        yields to it with a slide. Focus also expands the slot, keeping the
        trigger reachable by keyboard. */}
      <span className="text-muted-foreground/70 text-xs tabular-nums">
        {count}
      </span>
      <div className="w-0 overflow-hidden transition-[width] duration-150 ease-out group-hover/grow:w-6 group-focus-within/grow:w-6">
        <Popover open={popoverOpen} onOpenChange={setPopoverOpen}>
          <PopoverTrigger
            render={
              <Button
                variant="ghost"
                size="icon-xs"
                aria-label={t("common.edit")}
                disabled={busy}
              />
            }
          >
            <MoreHorizontal />
          </PopoverTrigger>
          <PopoverContent className="w-56 p-2" align="end">
            {renaming ? (
              <div className="flex items-center gap-1">
                <Input
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  className="h-7"
                  autoFocus
                  onKeyDown={(e) => {
                    if (e.key === "Enter") void commitRename()
                    if (e.key === "Escape") setRenaming(false)
                  }}
                />
                <Button variant="ghost" size="icon-sm" onClick={commitRename}>
                  <Check />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  onClick={() => setRenaming(false)}
                >
                  <X />
                </Button>
              </div>
            ) : (
              <div className="flex flex-col gap-0.5">
                <button
                  type="button"
                  className="hover:bg-muted flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm"
                  onClick={startRename}
                >
                  <Pencil className="size-3.5" />
                  {t("sessions.group.rename")}
                </button>
                <button
                  type="button"
                  className="text-destructive hover:bg-destructive/10 flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm"
                  onClick={confirmDelete}
                >
                  <Trash2 className="size-3.5" />
                  {t("sessions.group.delete")}
                </button>
              </div>
            )}
          </PopoverContent>
        </Popover>
      </div>
    </div>
  )
}
