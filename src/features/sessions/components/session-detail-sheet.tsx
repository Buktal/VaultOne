// Session detail Sheet — the signature element of the sessions view. Header
// carries the title (inline-renameable), source / project / timing / usage
// stats, the favorite star, and the move-to-group picker. Body renders the
// transcript as a three-voice timeline: assistant bubbles sit left, user
// bubbles right (mirrored, corner-cut toward the edge), tool / system rows
// span full width in the middle as the "workbench". Position encodes who spoke
// — assistant with a model badge, user in its own tone, tool rows collapsible,
// system muted. Messages collapse on click, expanded by default; tool rows
// collapse to their name, collapsed by default.
//
// Pure rendering — all state + queries live in useSessionsBrowser. The
// per-message expand state for tool rows is the only local state here; it is
// transient UI interaction that does not belong in the hook.

import dayjs from "dayjs"
import relativeTime from "dayjs/plugin/relativeTime"
import {
  Bot,
  Check,
  ChevronDown,
  ChevronRight,
  Copy,
  Info,
  Loader2,
  Pencil,
  Star,
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
          {/* Rename trigger: only the title text + pencil icon are clickable
            (w-fit), not the rest of the row. The pencil makes the affordance
            visible; the whole button is a native <button> so it stays
            keyboard-accessible. */}
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
            <SheetTitle className="text-base">
              <button
                type="button"
                onClick={onStartTitle}
                title={t("sessions.detail.renameHint")}
                className="hover:text-accent-brand-strong group flex w-fit max-w-full cursor-pointer items-center gap-1.5 rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
              >
                <span className="max-w-[24rem] truncate">
                  {s.title || t("sessions.untitled")}
                </span>
                <Pencil className="text-muted-foreground size-3.5 shrink-0 opacity-60 transition-opacity group-hover:opacity-100" />
              </button>
            </SheetTitle>
          )}
          <div className="text-muted-foreground flex flex-wrap items-center gap-x-3 gap-y-1 text-xs">
            <Badge variant="secondary">{sessionSourceLabel(s.source)}</Badge>
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
              className="h-7"
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
  // collapses it to a one-line summary.
  const [open, setOpen] = useState(true)
  const toggle = () => setOpen((o) => !o)
  switch (m.role) {
    case "assistant":
      return (
        <BaseRow
          icon={Bot}
          tone="assistant"
          time={m.ts}
          model={m.model}
          open={open}
          onToggle={toggle}
          copyText={m.content}
        >
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
          copyText={m.content}
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
          copyText={m.content}
        >
          <Content text={m.content} className={cn(!open && "line-clamp-1")} />
        </BaseRow>
      )
    default:
      return null
  }
}

function ToolRow({ message: m }: { message: SessionMessage }) {
  // Collapsed by default — tool output is the noisy part of a transcript, so
  // it hides behind the tool name until clicked (messages stay expanded).
  const [open, setOpen] = useState(false)
  const toggle = () => setOpen((o) => !o)
  const name = m.name || m.content?.split("\n")[0] || "tool"
  return (
    <div className="bg-muted/40 group rounded-md border border-dashed px-3 py-2 text-xs">
      {/* biome-ignore lint/a11y/useSemanticElements: collapse trigger must not
        be a <button> — the header embeds the copy <button>, and nested buttons
        are invalid HTML; div keeps the same keyboard contract. */}
      <div
        role="button"
        tabIndex={0}
        onClick={toggle}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault()
            toggle()
          }
        }}
        aria-expanded={open}
        className="hover:text-foreground text-muted-foreground flex w-full cursor-pointer items-center gap-1.5 text-left"
      >
        <Wrench className="size-3 shrink-0" />
        <span className="min-w-0 flex-1 truncate font-mono">{name}</span>
        <ChevronRight
          className={cn(
            "size-3 shrink-0 transition-transform",
            open && "rotate-90",
          )}
        />
        <CopyButton text={m.content} />
      </div>
      {open ? (
        <Content
          text={m.content}
          className="bg-background/60 text-muted-foreground mt-1.5 rounded p-2 font-mono"
        />
      ) : null}
    </div>
  )
}

/** Copy-to-clipboard for one message. Hidden until the row is hovered (or
 *  focused); shows a check for a moment after copying. Lives inside the
 *  row's collapse trigger, hence stopPropagation on click. */
function CopyButton({ text }: { text: string }) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)
  return (
    <button
      type="button"
      aria-label={t("sessions.detail.copyMessage")}
      title={t("sessions.detail.copyMessage")}
      onClick={(e) => {
        e.stopPropagation()
        void navigator.clipboard
          ?.writeText(text)
          .then(() => {
            setCopied(true)
            window.setTimeout(() => setCopied(false), 1500)
          })
          .catch(() => {})
      }}
      className="hover:text-foreground rounded p-0.5 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100"
    >
      {copied ? <Check className="size-3" /> : <Copy className="size-3" />}
    </button>
  )
}

function BaseRow({
  icon: Icon,
  tone,
  time,
  model,
  open,
  onToggle,
  copyText,
  children,
}: {
  icon: typeof Bot
  tone: "assistant" | "user" | "system"
  time: string
  model?: string | null
  open: boolean
  onToggle: () => void
  copyText: string
  children: ReactNode
}) {
  // Voice layout: assistant floats left, user floats right (mirrored so its
  // icon faces the edge), system stays full-width in the middle. The corner
  // cut toward each edge is the chat-bubble gesture; max-w = min(72ch, 80%)
  // caps line length on wide sheets and keeps narrow windows from filling the
  // whole row (72ch alone exceeds the content width once the sheet shrinks).
  // The whole bubble is the collapse toggle; the header row lines up icon +
  // time + model on the voice side (the user voice mirrors it to the bubble's
  // right edge) with the collapse chevron and copy button on the far side.
  const voiceClass =
    tone === "assistant"
      ? "mr-auto max-w-[min(72ch,80%)] rounded-lg rounded-bl-sm bg-muted/60"
      : tone === "user"
        ? "ml-auto max-w-[min(72ch,80%)] rounded-lg rounded-br-sm bg-accent-tint"
        : "bg-transparent"
  return (
    <>
      {/* biome-ignore lint/a11y/useSemanticElements: collapse trigger must
        not be a <button> — the header row embeds the copy <button>, and
        nested buttons are invalid HTML; div keeps the same keyboard
        contract. */}
      <div
        role="button"
        tabIndex={0}
        onClick={onToggle}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault()
            onToggle()
          }
        }}
        aria-expanded={open}
        className={cn(
          "group focus-visible:ring-ring/40 flex cursor-pointer px-3 py-2 text-left text-sm focus-visible:ring-2 focus-visible:outline-none",
          voiceClass,
        )}
      >
        <div className="min-w-0 flex-1">
          <div className="text-muted-foreground mb-1 flex items-center gap-1.5 text-[10px]">
            <div
              className={cn(
                "flex items-center gap-1.5",
                tone === "user" && "ml-auto flex-row-reverse",
              )}
            >
              <Icon
                className={cn(
                  "size-3.5 shrink-0",
                  tone === "system" && "text-muted-foreground/60",
                )}
              />
              {/* ts can be an empty string (codex/claude pass through whatever
              the source file has), so guard it like last_active_at. */}
              <span>{time ? dayjs(time).format("MM/DD HH:mm") : "—"}</span>
              {model ? (
                <Badge
                  variant="secondary"
                  className="h-4 px-1.5 font-mono text-[10px] leading-none"
                >
                  {model}
                </Badge>
              ) : null}
            </div>
            <div
              className={cn(
                "flex items-center gap-0.5",
                tone !== "user" && "ml-auto",
              )}
            >
              <ChevronDown
                className={cn(
                  "size-3 transition-transform",
                  !open && "rotate-90",
                )}
              />
              <CopyButton text={copyText} />
            </div>
          </div>
          {children}
        </div>
      </div>
    </>
  )
}

function Content({ text, className }: { text: string; className?: string }) {
  return (
    <div className={cn("whitespace-pre-wrap break-words", className)}>
      {text}
    </div>
  )
}
