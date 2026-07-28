// Whole-bar drag for the tucked mini-bar. startDragging() is a JS call (not
// data-tauri-drag-region), so it does NOT swallow the inner click — a press
// that moves > DRAG_THRESHOLD starts a window drag; a press that doesn't is a
// plain click. The caller reads `dragged` in its onClick to tell them apart.
//
// getCurrentWindow() is fetched lazily inside maybeDrag (not at module top), so
// importing this hook does not blow up a non-Tauri test environment.

import { getCurrentWindow } from "@tauri-apps/api/window"
import { type MouseEvent, useRef } from "react"

/** Square of the move distance (CSS px) that distinguishes a drag from a click.
 *  4px — small enough to feel instant, large enough that a click's natural
 *  jitter doesn't start a drag. */
const DRAG_THRESHOLD_SQ = 16

export function useTuckDrag() {
  const armed = useRef(false)
  const start = useRef({ x: 0, y: 0 })
  const dragged = useRef(false)

  /** Begin tracking a press (left button only). Resets `dragged` so a prior
   *  drag does not leak into this press's click decision. */
  const armDrag = (e: MouseEvent) => {
    if (e.button !== 0) return
    armed.current = true
    dragged.current = false
    start.current = { x: e.screenX, y: e.screenY }
  }

  /** If the press has moved past the threshold, start a window drag and mark
   *  `dragged` so the subsequent click is suppressed. No-op once dragging. */
  const maybeDrag = (e: MouseEvent) => {
    if (!armed.current || dragged.current) return
    const dx = e.screenX - start.current.x
    const dy = e.screenY - start.current.y
    if (dx * dx + dy * dy > DRAG_THRESHOLD_SQ) {
      dragged.current = true
      armed.current = false
      void getCurrentWindow().startDragging()
    }
  }

  /** Cancel tracking (mouse-up / leave) without starting a drag. */
  const disarm = () => {
    armed.current = false
  }

  return { armDrag, maybeDrag, disarm, dragged }
}
