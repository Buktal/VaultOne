// Close-requested dialog (ADR-0012). When the main window's close is
// intercepted with close_behavior = Ask, Rust emits `close-requested`; this
// component shows the minimize/quit dialog and resolves it via `confirmClose`.

import { listen } from "@tauri-apps/api/event"
import { useEffect, useState } from "react"
import { confirmClose } from "@/app/store/api"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

export function CloseRequestedDialog() {
  const [open, setOpen] = useState(false)
  const [remember, setRemember] = useState(false)

  useEffect(() => {
    const off = listen("close-requested", () => setOpen(true))
    return () => {
      off.then((unlisten) => unlisten())
    }
  }, [])

  const choose = (behavior: "minimize" | "quit") => {
    setOpen(false)
    void confirmClose(behavior, remember)
    setRemember(false)
  }

  return (
    <Dialog open={open} onOpenChange={(o) => setOpen(o)}>
      <DialogContent showClose={false}>
        <DialogHeader>
          <DialogTitle>关闭窗口</DialogTitle>
          <DialogDescription>
            VaultOne 可以最小化到托盘继续在后台采集。要最小化还是退出？
          </DialogDescription>
        </DialogHeader>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={remember}
            onChange={(e) => setRemember(e.target.checked)}
            className="size-4"
          />
          不再提示（记住本次选择）
        </label>
        <DialogFooter>
          <Button variant="outline" onClick={() => choose("quit")}>
            退出
          </Button>
          <Button onClick={() => choose("minimize")}>最小化到托盘</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
