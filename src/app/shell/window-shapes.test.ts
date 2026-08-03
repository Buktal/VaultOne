import { describe, expect, it } from "vitest"

import tauriConf from "../../../src-tauri/tauri.conf.json"
import { DEFAULT_SIZE, MIN_FULL } from "./window-shapes"

// The window-size constants here are the second source of truth for sizes the
// OS already owns in tauri.conf.json. If the two drift, the OS window opens (or
// is clamped by the OS) at one size while the front-end restores/clamps against
// another — a silent mismatch. These tests pin the two together so a change on
// one side that forgets the other fails here.

const mainWindow = tauriConf.app.windows[0]

describe("window-shapes ↔ tauri.conf.json", () => {
  it("DEFAULT_SIZE matches the main window's first-entry size", () => {
    expect(DEFAULT_SIZE).toEqual({ w: mainWindow.width, h: mainWindow.height })
  })

  it("MIN_FULL matches the main window's OS min size (front-end floor == OS floor)", () => {
    // The full-mode floor must equal the OS min-size: if they disagree, the
    // drag clamp and the OS min-size enforce different bounds.
    expect(MIN_FULL).toEqual({
      w: mainWindow.minWidth,
      h: mainWindow.minHeight,
    })
  })
})
