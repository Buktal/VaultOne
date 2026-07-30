// Preview a Library entry in the webview via convertFileSrc. The browser
// renders json / image / pdf / text natively; Markdown shows as source. The
// iframe is sandboxed without scripts so an uploaded HTML file cannot execute.

import { convertFileSrc } from "@tauri-apps/api/core"
import { useState } from "react"
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet"
import type { LibraryEntry } from "@/types/generated/bindings"

const IMAGE_EXTS = ["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp"]

function extOf(name: string) {
  return name.split(".").pop()?.toLowerCase() ?? ""
}

export function PreviewSheet({
  entry,
  onClose,
}: {
  entry: LibraryEntry
  onClose: () => void
}) {
  const url = convertFileSrc(entry.abs_path)
  const isImage = IMAGE_EXTS.includes(extOf(entry.name))
  const [scale, setScale] = useState(1)
  return (
    <Sheet open={true} onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="flex w-[640px] flex-col gap-3 sm:max-w-[640px]">
        <SheetHeader>
          <SheetTitle className="truncate">{entry.name}</SheetTitle>
        </SheetHeader>
        {isImage ? (
          // <img> fits the pane (max-w-full → no horizontal scroll) instead of
          // overflowing at native size inside an iframe. Ctrl+wheel zoom mirrors
          // the Win11 / browser image habit; other platforms scroll normally.
          <div
            className="border-border bg-background flex min-h-[60vh] w-full flex-1 items-center justify-center overflow-y-auto overflow-x-hidden rounded-md border p-2"
            onWheel={(e) => {
              if (!e.ctrlKey) return
              e.preventDefault()
              setScale((s) => Math.min(4, Math.max(0.5, s - e.deltaY * 0.002)))
            }}
          >
            <img
              src={url}
              alt={entry.name}
              style={{ transform: `scale(${scale})` }}
              className="max-w-full origin-top"
            />
          </div>
        ) : (
          <iframe
            src={url}
            title={entry.name}
            // allow-same-origin so the asset URL loads; no allow-scripts so an
            // uploaded HTML file cannot run.
            sandbox="allow-same-origin"
            className="border-border bg-background min-h-[60vh] w-full flex-1 rounded-md border"
          />
        )}
      </SheetContent>
    </Sheet>
  )
}
