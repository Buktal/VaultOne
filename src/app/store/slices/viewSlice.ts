// View state (ADR-0007: no react-router; desktop single-window, view switch via slice).

import { createSlice } from "@reduxjs/toolkit"

export type ViewId = "dashboard" | "pricing" | "settings"

interface ViewState {
  view: ViewId
}

const initialState: ViewState = { view: "dashboard" }

const viewSlice = createSlice({
  name: "view",
  initialState,
  reducers: {
    setView(state, action: { payload: ViewId }) {
      state.view = action.payload
    },
  },
})

export const { setView } = viewSlice.actions
export default viewSlice.reducer
