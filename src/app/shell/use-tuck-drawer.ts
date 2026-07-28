// Tucked hover-drawer state machine (two-step). Hovering the mini-bar slides
// out a Select-like trigger; clicking the trigger opens the device list. The
// caller owns the geometry (triggerH / listH) and the window-height sync
// (setTuckDrawer, from useLightweightTuck); this hook owns only the open/close
// sequencing and the 180ms anti-jitter leave delay.
//
// Anti-jitter: a tucked-window resize (drawer open/close) can fling the cursor
// briefly past the window edge → mouseleave → snap shut → mouseenter → reopen,
// looping. The 180ms close delay is cancelled by a re-enter (openDrawer),
// breaking the loop.
//
// This hook does NOT disarm the drag handle on leave — that stays the caller's
// job (the drag and drawer concerns are independent). See LightweightCard's
// onMouseLeave.

import { useEffect, useRef, useState } from "react"

export function useTuckDrawer({
  enabled,
  triggerH,
  listH,
  setTuckDrawer,
  phase,
}: {
  enabled: boolean
  triggerH: number
  listH: number
  setTuckDrawer: (h: number) => void
  phase: string
}) {
  const [drawerHover, setDrawerHover] = useState(false)
  const [listOpen, setListOpen] = useState(false)
  const leaveTimer = useRef<number | undefined>(undefined)

  const closeDrawer = () => {
    if (leaveTimer.current) {
      window.clearTimeout(leaveTimer.current)
      leaveTimer.current = undefined
    }
    setDrawerHover(false)
    setListOpen(false)
    setTuckDrawer(0)
  }

  const openDrawer = () => {
    if (leaveTimer.current) {
      window.clearTimeout(leaveTimer.current)
      leaveTimer.current = undefined
    }
    if (!enabled) return
    setDrawerHover(true)
    setTuckDrawer(triggerH)
  }

  /** Delayed close (see anti-jitter note): replaces an immediate close on
   *  mouseleave so a resize overshoot can't loop leave → close → enter → open. */
  const scheduleClose = () => {
    if (leaveTimer.current) window.clearTimeout(leaveTimer.current)
    leaveTimer.current = window.setTimeout(closeDrawer, 180)
  }

  const toggleList = () => {
    const next = !listOpen
    setListOpen(next)
    setTuckDrawer(next ? triggerH + listH : triggerH)
  }

  // Reset when leaving tucked (e.g. →大 to full): otherwise the drawer would
  // reopen the next time the mini-bar shows.
  useEffect(() => {
    if (phase !== "tucked") {
      if (leaveTimer.current) {
        window.clearTimeout(leaveTimer.current)
        leaveTimer.current = undefined
      }
      setDrawerHover(false)
      setListOpen(false)
      setTuckDrawer(0)
    }
  }, [phase, setTuckDrawer])

  return {
    drawerHover,
    listOpen,
    openDrawer,
    closeDrawer,
    scheduleClose,
    toggleList,
  }
}
