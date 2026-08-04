// Tests for the library browser.
//
// The navigation rules (splitEntryPath / upFromSubpath / buildBreadcrumb) are
// pure functions in ./derive, covered directly below — they encode the
// drill / go-up / breadcrumb invariants the view used to inline (architecture
// .md: "关键不变量用代码表达"). useLibraryBrowser itself just wires those
// derivations to React state + RTK Query; vitest runs in a pure node
// environment (see vitest.config.ts — no DOM), so renderHook is out of scope.
// What we guard at the bottom is that the hook module imports cleanly in node
// (it pulls @tauri-apps/api/webview + plugin-dialog + the tauri-specta API) —
// a regression that moves the getCurrentWebview() fetch to module top level
// would otherwise make the hook un-importable and zero-tested, the same
// failure mode that once hid the shell-hooks bug.

import { describe, expect, it } from "vitest"

import type { DeviceOption } from "./derive"
import { buildBreadcrumb, splitEntryPath, upFromSubpath } from "./derive"

describe("splitEntryPath", () => {
  it("splits a top-level entry into device id + empty rest", () => {
    expect(splitEntryPath("dev1")).toEqual({ deviceId: "dev1", rest: "" })
  })

  it("splits a nested entry into device id + relative subpath", () => {
    expect(splitEntryPath("dev1/projects/a.json")).toEqual({
      deviceId: "dev1",
      rest: "projects/a.json",
    })
  })
})

describe("upFromSubpath", () => {
  it("clears subpath at the root (empty input) without touching deviceScope", () => {
    expect(upFromSubpath("")).toEqual({ deviceScope: undefined, subpath: "" })
  })

  it("clears subpath for a single segment without touching deviceScope", () => {
    expect(upFromSubpath("projects")).toEqual({
      deviceScope: undefined,
      subpath: "",
    })
  })

  it("restores the first segment as deviceScope and clears subpath for two segments", () => {
    expect(upFromSubpath("dev1/projects")).toEqual({
      deviceScope: "dev1",
      subpath: "",
    })
  })

  it("drops only the last segment for three or more segments", () => {
    expect(upFromSubpath("dev1/projects/foo")).toEqual({
      deviceScope: "dev1",
      subpath: "projects",
    })
  })
})

describe("buildBreadcrumb", () => {
  const deviceOptions: DeviceOption[] = [
    { id: "dev1", label: "Device One" },
    { id: "dev2", label: "Device Two" },
  ]

  it("returns no crumbs at the root (empty subpath)", () => {
    expect(buildBreadcrumb("", deviceOptions)).toEqual([])
  })

  it("resolves the first segment against deviceOptions into a single device crumb", () => {
    expect(buildBreadcrumb("dev1", deviceOptions)).toEqual([
      { key: "dev1", label: "Device One", deviceScope: "dev1", subpath: "" },
    ])
  })

  it("falls back to the raw id when the device is not in deviceOptions", () => {
    expect(buildBreadcrumb("ghost", deviceOptions)).toEqual([
      { key: "ghost", label: "ghost", deviceScope: "ghost", subpath: "" },
    ])
  })

  it("emits one crumb per segment with nested navigation targets", () => {
    expect(buildBreadcrumb("dev1/projects/foo", deviceOptions)).toEqual([
      { key: "dev1", label: "Device One", deviceScope: "dev1", subpath: "" },
      {
        key: "dev1/projects",
        label: "projects",
        deviceScope: "dev1",
        subpath: "projects",
      },
      {
        key: "dev1/projects/foo",
        label: "foo",
        deviceScope: "dev1",
        subpath: "projects/foo",
      },
    ])
  })
})

describe("useLibraryBrowser imports in a non-Tauri (node) environment", () => {
  it("imports without throwing and exports a function", async () => {
    const mod = await import("./use-library-browser")
    expect(typeof mod.useLibraryBrowser).toBe("function")
  })

  it("exports the ALL scope sentinel the view needs for rendering", async () => {
    const mod = await import("./use-library-browser")
    expect(mod.ALL).toBe("__all__")
  })
})
