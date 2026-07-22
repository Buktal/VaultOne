// View state (ADR-0007: no react-router; desktop single-window, view switch via slice).
// Lightweight mode (ADR-0015): the same main window morphs between the full
// dashboard and an edge-docked glance card — not a second window. `mode` drives
// both the rendered shell (App branches on it) and the OS window's
// size / position / always-on-top (see useWindowMode).

import { createSlice } from "@reduxjs/toolkit"

export type ViewId = "dashboard" | "logs" | "pricing" | "settings"

/** Full dashboard ⇄ lightweight glance card (ADR-0015). Same window, two skins. */
export type WindowMode = "full" | "lightweight"

interface ViewState {
  view: ViewId
  mode: WindowMode
}

const initialState: ViewState = { view: "dashboard", mode: "full" }

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
  },
})

export const { setView, setMode } = viewSlice.actions
export default viewSlice.reducer
