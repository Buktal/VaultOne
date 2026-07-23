// Custom title bar (decorations:false). A full-width drag region sits on
// top; window controls live on the right only — the left is deliberately
// empty so it never duplicates the sidebar logo. Close reuses the existing
// CloseRequested routing (ADR-0012): appWindow.close() triggers the same
// minimize-to-tray / quit / ask flow as a system close.

import { getCurrentWindow } from "@tauri-apps/api/window"
import {
  AlignHorizontalJustifyEnd,
  Copy,
  Minus,
  PictureInPicture2,
  Square,
  X,
} from "lucide-react"
import { type ReactNode, useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { useAppDispatch } from "@/app/store/hooks"
import { setLightweightPhase, setMode } from "@/app/store/slices/viewSlice"
import { cn } from "@/lib/utils"

export function TitleBar() {
  const { t } = useTranslation()
  const appWindow = getCurrentWindow()
  const dispatch = useAppDispatch()
  const [maximized, setMaximized] = useState(false)

  useEffect(() => {
    void appWindow.isMaximized().then(setMaximized)
    const unlisten = appWindow.onResized(() => {
      void appWindow.isMaximized().then(setMaximized)
    })
    return () => {
      unlisten.then((u) => u())
    }
  }, [appWindow])

  return (
    <div
      data-tauri-drag-region
      className="flex h-9 shrink-0 select-none items-center justify-end gap-1 pe-2"
    >
      {/* Lightweight entries (ADR-0018): →中 (the 5-field glance card) and →小
          (the docked mini-bar). Both enter lightweight; the phase picks which
          sub-shape lands first. Decoupled from Close — entering is not closing. */}
      <CtrlButton
        onClick={() => {
          dispatch(setMode("lightweight"))
          dispatch(setLightweightPhase("expanded"))
        }}
        label={t("titlebar.lightweight")}
        className="me-1"
      >
        <PictureInPicture2 className="size-3.5" />
      </CtrlButton>
      <CtrlButton
        onClick={() => {
          dispatch(setMode("lightweight"))
          dispatch(setLightweightPhase("tucked"))
        }}
        label={t("titlebar.lightweightSmall")}
        className="me-1"
      >
        <AlignHorizontalJustifyEnd className="size-3.5" />
      </CtrlButton>
      <CtrlButton
        onClick={() => appWindow.minimize()}
        label={t("titlebar.minimize")}
      >
        <Minus className="size-3.5" />
      </CtrlButton>
      <CtrlButton
        onClick={() => appWindow.toggleMaximize()}
        label={t("titlebar.maximize")}
      >
        {maximized ? (
          <Copy className="size-3.5" />
        ) : (
          <Square className="size-3.5" />
        )}
      </CtrlButton>
      <CtrlButton
        onClick={() => appWindow.close()}
        label={t("titlebar.close")}
        className="hover:bg-destructive hover:text-white"
      >
        <X className="size-3.5" />
      </CtrlButton>
    </div>
  )
}

function CtrlButton({
  children,
  onClick,
  label,
  className,
}: {
  children: ReactNode
  onClick: () => void
  label: string
  className?: string
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      className={cn(
        "text-muted-foreground hover:bg-muted hover:text-foreground inline-flex size-7 items-center justify-center rounded-md transition-colors",
        className,
      )}
    >
      {children}
    </button>
  )
}
