import { describe, expect, it } from "vitest"

import {
  buildVerifyArgs,
  resolveVerifyResult,
} from "@/features/settings/use-sync-repo"

import type { VerifyReport } from "@/types/generated/bindings"

const FALLBACK = "request failed"

describe("buildVerifyArgs", () => {
  it("passes null args when already synced (backend re-reads stored PAT)", () => {
    expect(buildVerifyArgs(true, "https://x.git", "github_pat_abc")).toEqual({
      repoUrl: null,
      githubToken: null,
    })
  })

  it("trims the drafted url + token when not synced", () => {
    expect(
      buildVerifyArgs(false, "  https://github.com/o/r.git  ", "  pat_123  "),
    ).toEqual({
      repoUrl: "https://github.com/o/r.git",
      githubToken: "pat_123",
    })
  })

  it("ignores the synced draft entirely (null wins)", () => {
    expect(buildVerifyArgs(true, "  leftover  ", "  stale  ")).toEqual({
      repoUrl: null,
      githubToken: null,
    })
  })
})

describe("resolveVerifyResult", () => {
  const ok: VerifyReport = { ok: true, message: "reachable" }
  const fail: VerifyReport = { ok: false, message: "denied" }

  it("maps an error branch to the fallback banner", () => {
    expect(resolveVerifyResult({ error: new Error("join") }, FALLBACK)).toEqual(
      {
        ok: false,
        message: FALLBACK,
      },
    )
  })

  it("returns the data report on a resolved branch", () => {
    expect(resolveVerifyResult({ data: ok }, FALLBACK)).toBe(ok)
    expect(resolveVerifyResult({ data: fail }, FALLBACK)).toBe(fail)
  })

  it("collapses a data-less resolved branch to null", () => {
    expect(resolveVerifyResult({ data: undefined }, FALLBACK)).toBeNull()
    expect(resolveVerifyResult({}, FALLBACK)).toBeNull()
  })
})

describe("useSyncRepo hook module", () => {
  // Bare-import smoke: the hook transitively pulls use-freshness (which imports
  // @tauri-apps/api/event) and the RTK Query api. Neither touches an external
  // handle at module top level, so the module must import cleanly under vitest's
  // default node environment — the prerequisite for any future render-level test.
  it("imports without throwing and exports a function", async () => {
    const mod = await import("@/features/settings/use-sync-repo")
    expect(typeof mod.useSyncRepo).toBe("function")
  })
})
