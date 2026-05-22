import type { AppMode, Direction, EntryState } from "./types";

// ---------------------------------------------------------------------------
// Global mutable state
// ---------------------------------------------------------------------------

export let direction: Direction = "en-es";
export let isTranslating = false;
export let appMode: AppMode = "normal";
export let editingEntry: EntryState | null = null;
export let focusedEntry: EntryState | null = null;

export function setDirection(d: Direction): void { direction = d; }
export function setIsTranslating(v: boolean): void { isTranslating = v; }
export function setAppMode(m: AppMode): void { appMode = m; }
export function setEditingEntry(e: EntryState | null): void { editingEntry = e; }
export function setFocusedEntry(e: EntryState | null): void { focusedEntry = e; }

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

export const ENTRY_FOCUS_BG = "#1a1e2e";
export const HINTS_HISTORY =
  "↑/↓  navigate    E  edit    D  delete    C  copy translation    Shift+C  copy original    Esc  back";

// ---------------------------------------------------------------------------
// Pure direction helpers
// ---------------------------------------------------------------------------

export function dirLabel(): string {
  return direction === "en-es" ? "EN → ES" : "ES → EN";
}

export function dirParts(): [string, string] {
  return direction === "en-es" ? ["en", "es"] : ["es", "en"];
}

// EN→ES: steel blue  /  ES→EN: warm amber
export function dirColors(): { border: string; focusedBorder: string } {
  return direction === "en-es"
    ? { border: "#2266cc", focusedBorder: "#4499ff" }
    : { border: "#aa5500", focusedBorder: "#ff8833" };
}

export function inputTitle(): string {
  if (appMode === "editing") return ` ${dirLabel()} — Editing `;
  return ` ${dirLabel()} `;
}

export function hintsContent(mode: AppMode): string {
  if (mode === "editing") {
    return "Enter  save    Esc  cancel edit    Tab  switch dir    Ctrl+C  quit";
  }
  return "Enter  translate    Shift+Enter  new line    Tab  switch    Esc  clear/quit";
}
