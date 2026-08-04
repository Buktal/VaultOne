// Session detail Sheet — the signature element of the sessions view. Header
// carries the title (inline-renameable), source / project / timing / usage
// stats, the favorite star, and the move-to-group picker. Body renders the
// transcript as a role-differentiated timeline (assistant with a model badge,
// user in its own tone, tool rows collapsible, system muted).
//
// Pure rendering — all state + queries live in useSessionsBrowser. The
// per-message expand state for tool rows is the only local state here; it is
// transient UI interaction that does not belong in the hook.

import dayjs from "dayjs"
import relativeTime from "dayjs/plugin/relativeTime"
import {
  Bot,
  ChevronRight,
  Info,
  Loader2,
  Star,
  StarOff,
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
        className="flex w-[640px] flex-col gap-0 overflow-hidden p-0 sm:max-w-[640px]"
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
              {favorited ? <Star /> : <StarOff />}
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
          favorited={favorited}
          onFavorite={onToggleFavorite}
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
  favorited,
  onFavorite,
  messages,
  loading,
  error,
  onRefresh,
}: {
  favorited: boolean
  onFavorite: () => void
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
    // Favorited but empty → original is still being collected (~30s next
    // collect). Not favorited → offer to favorite so the original gets
    // collected at all (collect only writes JSONL for favorited sessions).
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center p-6">
        <EmptyState
          icon={Bot}
          title={
            favorited
              ? t("sessions.detail.transcriptCollecting")
              : t("sessions.detail.transcriptLocked")
          }
          description={
            favorited
              ? t("sessions.detail.transcriptCollectingHint")
              : t("sessions.detail.transcriptLockedHint")
          }
          action={
            favorited
              ? {
                  label: t("sessions.detail.refresh"),
                  onClick: onRefresh,
                }
              : {
                  label: t("sessions.row.favorite"),
                  onClick: onFavorite,
                }
          }
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
  switch (m.role) {
    case "assistant":
      return (
        <BaseRow icon={Bot} tone="assistant" time={m.ts}>
          {m.model ? (
            <Badge variant="secondary" className="mb-1 font-mono text-[10px]">
              {m.model}
            </Badge>
          ) : null}
          <Content text={m.content} />
        </BaseRow>
      )
    case "user":
      return (
        <BaseRow icon={UserIcon} tone="user" time={m.ts}>
          <Content text={m.content} />
        </BaseRow>
      )
    case "tool":
      return <ToolRow message={m} />
    case "system":
      return (
        <BaseRow icon={Info} tone="system" time={m.ts}>
          <Content text={m.content} />
        </BaseRow>
      )
    default:
      return null
  }
}

function ToolRow({ message: m }: { message: SessionMessage }) {
  const [open, setOpen] = useState(false)
  const name = m.name || m.content?.split("\n")[0] || "tool"
  return (
    <div className="bg-muted/40 rounded-md border border-dashed px-2.5 py-1.5 text-xs">
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
  children,
}: {
  icon: typeof Bot
  tone: "assistant" | "user" | "system"
  time: string
  children: ReactNode
}) {
  const toneClass =
    tone === "assistant"
      ? "bg-muted/60"
      : tone === "user"
        ? "bg-accent-tint"
        : "bg-transparent"
  return (
    <div
      className={cn("flex gap-2 rounded-md px-2.5 py-1.5 text-sm", toneClass)}
    >
      <Icon
        className={cn(
          "mt-0.5 size-3.5 shrink-0",
          tone === "system" && "text-muted-foreground/60",
        )}
      />
      <div className="min-w-0 flex-1">
        <div
          className={cn(
            "text-muted-foreground mb-0.5 flex items-center gap-1 text-[10px]",
            tone === "system" && "text-[10px]",
          )}
        >
          <span>{dayjs(time).format("MM/DD HH:mm")}</span>
        </div>
        {children}
      </div>
    </div>
  )
}

function Content({ text, className }: { text: string; className?: string }) {
  return (
    <div className={cn("whitespace-pre-wrap break-words", className)}>
      {text}
    </div>
  )
}
