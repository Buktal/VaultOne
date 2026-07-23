// Preferences (ADR-0012) — hand-mirrored from Rust until tauri-specta regenerates
// `bindings.ts`. Source of truth: `commands::Preferences` / `config::CloseBehavior`.

/** Window-close behavior. Matches Rust `CloseBehavior` (serde snake_case). */
export type CloseBehavior = "ask" | "minimize" | "quit"

/** Display language. Matches Rust `Language` (serde lowercase; ADR-0016). */
export type Language = "en" | "zh" | "ja"

/** User-tunable preferences surfaced in the Settings「通用」card. */
export interface Preferences {
  close_behavior: CloseBehavior
  collect_interval_secs: number
  /** Push-to-sync interval (seconds, Synced only). ADR-0014. */
  push_interval_secs: number
  /** Display language (ADR-0016). Default `en`; per-device, not synced. */
  language: Language
}
