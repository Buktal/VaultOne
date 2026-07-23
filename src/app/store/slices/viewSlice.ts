// View state (ADR-0007: no react-router; desktop single-window, view switch via slice).
// Lightweight mode (ADR-0015): the same main window morphs between the full
// dashboard and an edge-docked glance card — not a second window. `mode` drives
// both the rendered shell (App branches on it) and the OS window's
// size / position / always-on-top (see useWindowMode).

import { createSlice } from "@reduxjs/toolkit"

export type ViewId = "dashboard" | "logs" | "pricing" | "settings"

/** Full dashboard ⇄ lightweight glance card (ADR-0018). Same window, two skins. */
export type WindowMode = "full" | "lightweight"

/** The two lightweight sub-shapes (ADR-0018): the 5-field glance card
 *  (expanded / "中") and the docked mini-bar (tucked / "小"). Hoisted into the
 *  store so the full-mode title bar can enter lightweight directly in either
 *  sub-shape (→中 or →小), and so tuck/expand are plain dispatches. */
export type LightweightPhase = "expanded" | "tucked"

interface ViewState {
  view: ViewId
  mode: WindowMode
  lightweightPhase: LightweightPhase
}

const initialState: ViewState = {
  view: "dashboard",
  mode: "full",
  lightweightPhase: "expanded",
}

const viewSlice = createSlice({
  name: "view",
  initialState,
  reducers: {
    setView(state, action: { payload: ViewId }) {
      state.view = action.payload
    },
    setMode(state, action: { payload: WindowMode }) {
      state.mode = action.payload
    },
    setLightweightPhase(state, action: { payload: LightweightPhase }) {
      state.lightweightPhase = action.payload
    },
  },
})

export const { setView, setMode, setLightweightPhase } = viewSlice.actions
export default viewSlice.reducer
