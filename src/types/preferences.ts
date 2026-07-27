// Preferences — hand-mirrored from Rust until tauri-specta regenerates
// `bindings.ts`. Source of truth: `commands::Preferences` / `config::CloseBehavior`.

/** Window-close behavior. Matches Rust `CloseBehavior` (serde snake_case). */
export type CloseBehavior = "ask" | "minimize" | "quit"

/** Display language. Matches Rust `Language` (serde lowercase). */
export type Language = "en" | "zh" | "ja"

/** How the lightweight half-icon expands. Matches Rust `LightweightExpand`
 *  (serde snake_case). */
export type LightweightExpand = "click" | "hover"

/** Color skin (multi-skin theming). Matches Rust `Skin` (serde snake_case).
 *  `neutral` is the default and needs no data-skin attribute on <html>. */
export type Skin = "neutral" | "sage" | "azure" | "crimson" | "mauve"

/** User-tunable preferences surfaced in the Settings「通用」card. */
export interface Preferences {
  close_behavior: CloseBehavior
  collect_interval_secs: number
  /** Push-to-sync interval (seconds, Synced only). */
  push_interval_secs: number
  /** Display language. Default `en`; per-device, not synced. */
  language: Language
  /** How the lightweight half-icon expands. Default `click`. */
  lightweight_expand: LightweightExpand
  /** Color skin (multi-skin theming). Default `neutral`; per-device, not synced. */
  skin: Skin
}
