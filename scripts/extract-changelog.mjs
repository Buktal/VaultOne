// Extract this release's section from CHANGELOG.md and print it to stdout.
// Used by release.yml as the GitHub Release body, which tauri-action copies
// into the updater's latest.json `notes` (shown in the in-app UpdateCard).
//
// Env: RELEASE_TAG (e.g. "v1.0.0") — the leading "v" is stripped.
// CHANGELOG.md follows Keep a Changelog: a "## [x.y.z] - date" heading per
// release, terminated by the next "## [" heading or a "[x.y.z]:" link line.

import { readFileSync } from "node:fs"

const tag = (process.env.RELEASE_TAG ?? "").replace(/^v/, "")
const changelog = readFileSync("CHANGELOG.md", "utf8")

const re = new RegExp(
  `## \\[${tag}\\][^\\n]*\\n([\\s\\S]*?)(?=\\n## \\[|\\n\\[${tag}\\]:|$)`,
)
const match = changelog.match(re)

process.stdout.write(match ? match[1].trim() : "See CHANGELOG.md.")
