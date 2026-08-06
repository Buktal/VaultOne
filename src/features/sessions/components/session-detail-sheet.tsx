// Session detail Sheet — the signature element of the sessions view. Header
// carries the title (inline-renameable), source / project / timing / usage
// stats, the favorite star, and the move-to-group picker. Body renders the
// transcript as a three-voice timeline: assistant bubbles sit left, user
// bubbles right (mirrored, corner-cut toward the edge), tool / system rows
// span full width in the middle as the "workbench". Position encodes who spoke
// — assistant with a model badge, user in its own tone, tool rows collapsible,
// system muted. Every message collapses on click (expanded by default).
//
// Pure rendering — all state + queries live in useSessionsBrowser. The
// per-message expand state for tool rows is the only local state here; it is
// transient UI interaction that does not belong in the hook.

import dayjs from "dayjs"
import relativeTime from "dayjs/plugin/relativeTime"
import {
  Bot,
  ChevronDown,
  ChevronRight,
  Info,
  Loader2,
  Star,
  Terminal,
  User as UserIcon,
  Wrench,
} from "lucide-react"
import { type ReactNode, useState } from "react"
import { useTranslation } from "react-i18next"
import { EmptyState } from "@/components/empty-state"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { ScrollArea } from "@/components/ui/scroll-area"
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
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet"
import { formatCost, formatInt, formatTokens } from "@/lib/format"
import { cn } from "@/lib/utils"
import type {
  SessionGroup,
  SessionMessage,
  SessionRow,
} from "@/types/generated/bindings"
import { sessionSourceLabel } from "../source-labels"

dayjs.extend(relativeTime)

/** A group-picker entry plus the special "no group" / "leave as-is" options.
 *  Mirrors the sidebar's ALL/UNGROUPED sentinels but the detail picker only
 *  needs "none" (clear the assignment) vs a real group. */
const NO_GROUP = "__none__"

export interface SessionDetailSheetProps {
  session: SessionRow
  favorited: boolean
  onClose: () => void
  onToggleFavorite: () => void
  // title rename
  editTitle: boolean
  titleDraft: string
  onTitleDraft: (v: string) => void
  onStartTitle: () => void
  onCancelTitle: () => void
  onCommitTitle: () => void
  // group assignment
  trackGroups: SessionGroup[]
  currentGroupId: string
  onSetGroup: (groupId: string | null) => void
  // transcript
  transcript: SessionMessage[]
  transcriptLoading: boolean
  transcriptError: unknown
  onRefreshTranscript: () => void
  // device label (for the source line)
  deviceLabel: (id: string) => string
}

export function SessionDetailSheet(props: SessionDetailSheetProps) {
  const { t } = useTranslation()
  const {
    session: s,
    favorited,
    onClose,
    onToggleFavorite,
    editTitle,
    titleDraft,
    onTitleDraft,
    onStartTitle,
    onCancelTitle,
    onCommitTitle,
    trackGroups,
    currentGroupId,
    onSetGroup,
    transcript,
    transcriptLoading,
    transcriptError,
    onRefreshTranscript,
    deviceLabel,
  } = props

  return (
    <Sheet open={true} onOpenChange={(o) => !o && onClose()}>
      <SheetContent
        showClose={false}
        // Width tracks the window: `100vw - 32rem` leaves the sidebar + the
        // full title column of the list visible in the background (~70% of
        // the window), so the user can still tell which session is open.
        // `min-w` keeps narrow windows from squeezing the transcript below a
        // readable size; `sm:max-w-none` overrides the sheet primitive's
        // default 24rem cap.
        className="flex w-[calc(100vw-32rem)] min-w-[32rem] flex-col gap-0 overflow-hidden p-0 sm:max-w-none"
      >
        {/* Header: title + meta + actions */}
        <SheetHeader className="border-border gap-2 border-b p-4 pr-10">
          {editTitle ? (
            <div className="flex items-center gap-1">
              <Input
                value={titleDraft}
                onChange={(e) => onTitleDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") onCommitTitle()
                  if (e.key === "Escape") onCancelTitle()
                }}
                className="h-7"
                autoFocus
              />
              <Button variant="ghost" size="sm" onClick={onCommitTitle}>
                {t("common.save")}
              </Button>
              <Button variant="ghost" size="icon-sm" onClick={onCancelTitle}>
                {t("common.cancel")}
              </Button>
            </div>
          ) : (
            <SheetTitle
              className="truncate hover:text-accent-brand-strong cursor-pointer text-base"
              onClick={onStartTitle}
              title={t("sessions.detail.renameHint")}
            >
              {s.title || t("sessions.untitled")}
            </SheetTitle>
          )}
          <div className="text-muted-foreground flex flex-wrap items-center gap-x-3 gap-y-1 text-xs">
            <span>{sessionSourceLabel(s.source)}</span>
            <span className="truncate" title={s.project_dir}>
              {s.project_dir || "—"}
            </span>
            <span>{deviceLabel(s.device_id)}</span>
            <span title={dayjs(s.last_active_at).format("YYYY-MM-DD HH:mm")}>
              {s.last_active_at ? dayjs(s.last_active_at).fromNow() : "—"}
            </span>
          </div>
          <div className="text-muted-foreground flex items-center gap-3 text-xs tabular-nums">
            <span>
              {formatInt(s.request_count)} {t("sessions.col.requests")}
            </span>
            <span>{formatTokens(s.total_tokens)} tok</span>
            <span>{formatCost(s.total_cost_usd)}</span>
          </div>
          <div className="flex items-center gap-2 pt-1">
            <Button
              variant={favorited ? "default" : "outline"}
              size="sm"
              onClick={onToggleFavorite}
            >
              <Star className={cn("size-4", favorited && "fill-current")} />
              {favorited
                ? t("sessions.row.unfavorite")
                : t("sessions.row.favorite")}
            </Button>
            <Select
              value={currentGroupId || NO_GROUP}
              onValueChange={(v) =>
                onSetGroup(v === NO_GROUP ? null : (v ?? null))
              }
            >
              <SelectTrigger className="h-8 w-48" size="sm">
                <SelectValue>
                  {(val: string) => {
                    if (val === NO_GROUP) return t("sessions.detail.noGroup")
                    return (
                      trackGroups.find((g) => g.id === val)?.name ??
                      t("sessions.detail.noGroup")
                    )
                  }}
                </SelectValue>
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NO_GROUP}>
                  {t("sessions.detail.noGroup")}
                </SelectItem>
                {trackGroups.map((g) => (
                  <SelectItem key={g.id} value={g.id}>
                    {g.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </SheetHeader>

        {/* Body: transcript timeline */}
        <TranscriptBody
          messages={transcript}
          loading={transcriptLoading}
          error={transcriptError}
          onRefresh={onRefreshTranscript}
        />
      </SheetContent>
    </Sheet>
  )
}

function TranscriptBody({
  messages,
  loading,
  error,
  onRefresh,
}: {
  messages: SessionMessage[]
  loading: boolean
  error: unknown
  onRefresh: () => void
}) {
  const { t } = useTranslation()

  if (loading) {
    return (
      <div className="text-muted-foreground flex min-h-0 flex-1 items-center justify-center gap-2 p-8 text-sm">
        <Loader2 className="size-4 animate-spin" />
        {t("common.loading")}
      </div>
    )
  }
  if (error) {
    return (
      <div className="text-muted-foreground flex min-h-0 flex-1 items-center justify-center p-8 text-sm">
        {t("common.loadFailed", { detail: "" })}
      </div>
    )
  }
  if (messages.length === 0) {
    // Empty = the transcript isn't in the db yet. Every session's messages land
    // in `session_messages` regardless of favorite status, so this is a
    // collection lag (the next collect picks them up), not a favorite gate.
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center p-6">
        <EmptyState
          icon={Bot}
          title={t("sessions.detail.transcriptCollecting")}
          description={t("sessions.detail.transcriptCollectingHint")}
          action={{
            label: t("sessions.detail.refresh"),
            onClick: onRefresh,
          }}
        />
      </div>
    )
  }
  return (
    <ScrollArea className="min-h-0 flex-1">
      <div className="flex flex-col gap-2 p-4">
        {messages.map((m) => (
          <MessageRow key={m.uuid} message={m} />
        ))}
      </div>
    </ScrollArea>
  )
}

function MessageRow({ message: m }: { message: SessionMessage }) {
  // Per-message collapse state, expanded by default. Clicking a bubble
  // collapses it to a one-line summary (the model badge hides too, keeping
  // the collapsed strip readable).
  const [open, setOpen] = useState(true)
  const toggle = () => setOpen((o) => !o)
  switch (m.role) {
    case "assistant":
      return (
        <BaseRow
          icon={Bot}
          tone="assistant"
          time={m.ts}
          open={open}
          onToggle={toggle}
        >
          {open && m.model ? (
            <Badge variant="secondary" className="mb-1 font-mono text-[10px]">
              {m.model}
            </Badge>
          ) : null}
          <Content text={m.content} className={cn(!open && "line-clamp-1")} />
        </BaseRow>
      )
    case "user":
      return (
        <BaseRow
          icon={UserIcon}
          tone="user"
          time={m.ts}
          open={open}
          onToggle={toggle}
        >
          <Content text={m.content} className={cn(!open && "line-clamp-1")} />
        </BaseRow>
      )
    case "tool":
      return <ToolRow message={m} />
    case "system":
      return (
        <BaseRow
          icon={Info}
          tone="system"
          time={m.ts}
          open={open}
          onToggle={toggle}
        >
          <Content text={m.content} className={cn(!open && "line-clamp-1")} />
        </BaseRow>
      )
    default:
      return null
  }
}

function ToolRow({ message: m }: { message: SessionMessage }) {
  // Expanded by default, like every other message; clicking the header
  // collapses the output back to the tool name alone.
  const [open, setOpen] = useState(true)
  const name = m.name || m.content?.split("\n")[0] || "tool"
  return (
    <div className="bg-muted/40 rounded-md border border-dashed px-3 py-2 text-xs">
      <button
        type="button"
        className="hover:text-foreground text-muted-foreground flex w-full items-center gap-1.5 text-left"
        onClick={() => setOpen((o) => !o)}
      >
        <Wrench className="size-3 shrink-0" />
        <Terminal className="size-3 shrink-0" />
        <span className="font-mono">{name}</span>
        <ChevronRight
          className={cn(
            "ml-auto size-3 shrink-0 transition-transform",
            open && "rotate-90",
          )}
        />
      </button>
      {open ? (
        <Content
          text={m.content}
          className="text-muted-foreground mt-1 font-mono"
        />
      ) : null}
    </div>
  )
}

function BaseRow({
  icon: Icon,
  tone,
  time,
  open,
  onToggle,
  children,
}: {
  icon: typeof Bot
  tone: "assistant" | "user" | "system"
  time: string
  open: boolean
  onToggle: () => void
  children: ReactNode
}) {
  // Voice layout: assistant floats left, user floats right (mirrored so its
  // icon faces the edge), system stays full-width in the middle. The corner
  // cut toward each edge is the chat-bubble gesture; max-w caps line length
  // once the sheet fills the window. The whole bubble is the collapse toggle
  // — the timestamp row carries the chevron (down = expanded, rotated right
  // = collapsed).
  const voiceClass =
    tone === "assistant"
      ? "mr-auto max-w-[72ch] rounded-lg rounded-bl-sm bg-muted/60"
      : tone === "user"
        ? "ml-auto max-w-[72ch] flex-row-reverse rounded-lg rounded-br-sm bg-accent-tint"
        : "bg-transparent"
  return (
    <button
      type="button"
      onClick={onToggle}
      aria-expanded={open}
      className={cn(
        "focus-visible:ring-ring/40 flex cursor-pointer gap-2 px-3 py-2 text-left text-sm focus-visible:ring-2 focus-visible:outline-none",
        voiceClass,
      )}
    >
      <Icon
        className={cn(
          "mt-0.5 size-3.5 shrink-0",
          tone === "system" && "text-muted-foreground/60",
        )}
      />
      <div className="min-w-0 flex-1">
        <div className="text-muted-foreground mb-0.5 flex items-center gap-1 text-[10px]">
          {/* ts can be an empty string (codex/claude pass through whatever the
            source file has), so guard it like last_active_at elsewhere. */}
          <span>{time ? dayjs(time).format("MM/DD HH:mm") : "—"}</span>
          <ChevronDown
            className={cn(
              "ml-auto size-3 transition-transform",
              !open && "rotate-90",
            )}
          />
        </div>
        {children}
      </div>
    </button>
  )
}

function Content({ text, className }: { text: string; className?: string }) {
  return (
    <div className={cn("whitespace-pre-wrap break-words", className)}>
      {text}
    </div>
  )
}
