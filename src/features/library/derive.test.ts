import { describe, expect, it } from "vitest"

import { shouldThemeRender } from "./derive"

describe("shouldThemeRender", () => {
  it("renders text extensions theme-side, case-insensitively", () => {
    for (const name of [
      "notes.md",
      "README.MD",
      "config.json",
      "archive.JSON",
      "doc.markdown",
      "log.txt",
      "session.log",
    ]) {
      expect(shouldThemeRender(name), name).toBe(true)
    }
  })

  it("keeps iframe rendering for html / pdf / svg / unknown / extensionless", () => {
    for (const name of [
      "page.html",
      "manual.pdf",
      "image.svg",
      "script.py",
      "no-extension",
      "archive.tar.gz",
      ".gitkeep",
    ]) {
      expect(shouldThemeRender(name), name).toBe(false)
    }
  })
})
