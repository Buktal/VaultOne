import path from "node:path"
import { fileURLToPath } from "node:url"
import { defineConfig } from "vitest/config"

const __dirname = path.dirname(fileURLToPath(import.meta.url))

export default defineConfig({
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  test: {
    // Pure-function tests only — no DOM needed, so the default node environment
    // is enough and keeps the runner fast.
    include: ["src/**/*.test.ts"],
  },
})
