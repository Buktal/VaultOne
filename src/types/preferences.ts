// Preferences (ADR-0012) — hand-mirrored from Rust until tauri-specta regenerates
// `bindings.ts`. Source of truth: `commands::Preferences` / `config::CloseBehavior`.

/** Window-close behavior. Matches Rust `CloseBehavior` (serde snake_case). */
export type CloseBehavior = "ask" | "minimize" | "quit"

/** User-tunable preferences surfaced in the Settings「通用」card. */
export interface Preferences {
  close_behavior: CloseBehavior
  collect_interval_secs: number
}
