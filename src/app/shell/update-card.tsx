// Update indicator + popover card + Settings row (ADR-0017).
//
// UpdateIndicator: the footer ⓘ. Shows only when a probe surfaced a new
//   version (available / downloading / ready / failed); clicking opens the
//   UpdateCard popover, which renders one body per status. [稍后] and
//   post-action close it via the controlled `open` state owned here.
// UpdateControl: the Settings「版本与更新」row — current version + a manual
//   「检查更新」button + a one-line status echo.

import {
  CircleArrowUp,
  ExternalLink,
  Loader2,
  PartyPopper,
  RotateCw,
} from "lucide-react"
import { useState } from "react"
import { useTranslation } from "react-i18next"
import { useUpdateCheck } from "@/app/shell/use-update-check"
import { useAppInfoQuery } from "@/app/store/api"
import { useAppSelector } from "@/app/store/hooks"
import type { UpdateStatus } from "@/app/store/slices/updateSlice"
import { Button } from "@/components/ui/button"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"

const NOTES_PREVIEW_LINES = 5

/** Strip markdown emphasis/list markers and keep the first few lines. */
function summarizeNotes(notes: string): string {
  return notes
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.length > 0 && !l.startsWith("<!--"))
    .slice(0, NOTES_PREVIEW_LINES)
    .map((l) =>
      l
        .replace(/^[-*+]\s+/, "")
        .replace(/^#+\s*/, "")
        .replace(/[*_`]/g, "")
        .trim(),
    )
    .join("\n")
}

export function UpdateIndicator() {
  const { t } = useTranslation()
  const status = useAppSelector((s) => s.update.status)
  const { applyUpdate, restartNow, openReleases } = useUpdateCheck()
  const [open, setOpen] = useState(false)

  // Idle / checking / up-to-date never surface the indicator (ADR-0017) —
  // only a real new-version state shows the ⓘ.
  if (status === "idle" || status === "checking" || status === "up-to-date") {
    return null
  }

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <button
            type="button"
            aria-label={t("update.available.aria")}
            className="text-primary relative inline-flex size-3.5 items-center justify-center"
          />
        }
      >
        <CircleArrowUp className="size-3.5" />
        <span className="bg-primary absolute -right-1 -top-1 size-1.5 rounded-full" />
      </PopoverTrigger>
      <PopoverContent
        align="start"
        side="top"
        className="text-muted-foreground flex w-72 flex-col gap-2 p-4 text-xs"
      >
        <UpdateCardBody
          status={status}
          onApply={() => void applyUpdate()}
          onRestart={() => void restartNow()}
          onOpenReleases={() => {
            void openReleases()
            setOpen(false)
          }}
          onDismiss={() => setOpen(false)}
        />
      </PopoverContent>
    </Popover>
  )
}

function UpdateCardBody({
  status,
  onApply,
  onRestart,
  onOpenReleases,
  onDismiss,
}: {
  status: UpdateStatus
  onApply: () => void
  onRestart: () => void
  onOpenReleases: () => void
  onDismiss: () => void
}) {
  const { t } = useTranslation()
  const version = useAppSelector((s) => s.update.version)
  const currentVersion = useAppSelector((s) => s.update.currentVersion)
  const notes = useAppSelector((s) => s.update.notes)
  const error = useAppSelector((s) => s.update.error)
  const downloadedBytes = useAppSelector((s) => s.update.downloadedBytes)
  const totalBytes = useAppSelector((s) => s.update.totalBytes)

  if (status === "available") {
    return (
      <>
        <div className="text-foreground flex items-center gap-1.5 font-medium">
          <CircleArrowUp className="text-primary size-3.5" />
          {t("update.available.found", { version })}
        </div>
        {currentVersion ? (
          <p>{t("update.current", { version: currentVersion })}</p>
        ) : null}
        {notes ? (
          <p className="line-clamp-5 whitespace-pre-line break-words">
            {summarizeNotes(notes)}
          </p>
        ) : null}
        <div className="flex gap-2 pt-1">
          <Button size="xs" onClick={onApply}>
            <CircleArrowUp />
            {t("update.updateNow")}
          </Button>
          <Button size="xs" variant="ghost" onClick={onDismiss}>
            {t("update.later")}
          </Button>
        </div>
      </>
    )
  }

  if (status === "downloading") {
    const pct =
      totalBytes > 0
        ? Math.min(100, Math.round((downloadedBytes / totalBytes) * 100))
        : null
    return (
      <>
        <div className="text-foreground flex items-center gap-1.5 font-medium">
          <Loader2 className="size-3.5 animate-spin" />
          {t("update.downloading")}
        </div>
        <p>
          {pct !== null
            ? t("update.downloaded", { pct })
            : t("update.pleaseWait")}
        </p>
      </>
    )
  }

  if (status === "ready") {
    return (
      <>
        <div className="text-foreground flex items-center gap-1.5 font-medium">
          <PartyPopper className="text-primary size-3.5" />
          {t("update.ready")}
        </div>
        <p>{t("update.restartToInstall")}</p>
        <Button size="xs" onClick={onRestart}>
          <RotateCw />
          {t("update.restartNow")}
        </Button>
      </>
    )
  }

  // failed — Manual Fallback (ADR-0017).
  return (
    <>
      <div className="text-destructive flex items-center gap-1.5 font-medium">
        <CircleArrowUp className="size-3.5" />
        {t("update.failed")}
      </div>
      {error ? <p>{error}</p> : null}
      <p>{t("update.manualHint")}</p>
      <div className="flex gap-2 pt-1">
        <Button size="xs" variant="outline" onClick={onOpenReleases}>
          <ExternalLink />
          {t("update.openGithub")}
        </Button>
        <Button size="xs" variant="ghost" onClick={onDismiss}>
          {t("common.close")}
        </Button>
      </div>
    </>
  )
}

/** Settings「版本与更新」row: current version + manual check + status echo. */
export function UpdateControl() {
  const { t } = useTranslation()
  const status = useAppSelector((s) => s.update.status)
  const { checkNow } = useUpdateCheck()
  const { data: info } = useAppInfoQuery()
  const checking = status === "checking"

  return (
    <div className="flex flex-wrap items-center gap-2">
      {info?.version ? (
        <span className="text-muted-foreground">
          {t("update.current", { version: info.version })}
        </span>
      ) : null}
      <Button
        size="sm"
        variant="outline"
        disabled={checking}
        onClick={() => void checkNow()}
      >
        {checking ? <Loader2 className="animate-spin" /> : <RotateCw />}
        {checking ? t("update.checking") : t("update.checkNow")}
      </Button>
      {status === "up-to-date" ? (
        <span className="text-muted-foreground text-xs">
          {t("update.upToDate")}
        </span>
      ) : null}
      {status === "available" ? (
        <span className="text-primary text-xs">
          {t("update.available.hint")}
        </span>
      ) : null}
    </div>
  )
}
