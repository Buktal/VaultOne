// Custom title bar (decorations:false). A full-width drag region sits on
// top; window controls live on the right only — the left is deliberately
// empty so it never duplicates the sidebar logo. Close reuses the existing
// CloseRequested routing (ADR-0012): appWindow.close() triggers the same
// minimize-to-tray / quit / ask flow as a system close.

import { getCurrentWindow } from "@tauri-apps/api/window"
import { Copy, Minus, Square, X } from "lucide-react"
import { type ReactNode, useEffect, useState } from "react"
import { cn } from "@/lib/utils"

export function TitleBar() {
  const appWindow = getCurrentWindow()
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
      <CtrlButton onClick={() => appWindow.minimize()} label="最小化">
        <Minus className="size-3.5" />
      </CtrlButton>
      <CtrlButton onClick={() => appWindow.toggleMaximize()} label="最大化">
        {maximized ? (
          <Copy className="size-3.5" />
        ) : (
          <Square className="size-3.5" />
        )}
      </CtrlButton>
      <CtrlButton
        onClick={() => appWindow.close()}
        label="关闭"
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
