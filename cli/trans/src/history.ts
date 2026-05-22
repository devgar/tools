import { BoxRenderable, TextRenderable, TextAttributes } from "@opentui/core";
import type { EntryState } from "./types";
import {
  focusedEntry,
  setFocusedEntry,
  appMode,
  ENTRY_FOCUS_BG,
  HINTS_HISTORY,
  hintsContent,
} from "./state";
import { renderer, historyBox, textarea, hintsText } from "./layout";

// ---------------------------------------------------------------------------
// Entries list
// ---------------------------------------------------------------------------

export const entries: EntryState[] = [];

// ---------------------------------------------------------------------------
// Focus management
// ---------------------------------------------------------------------------

export function focusHistoryEntry(entry: EntryState): void {
  if (focusedEntry && focusedEntry !== entry) {
    focusedEntry.entryBox.backgroundColor = undefined;
  }
  setFocusedEntry(entry);
  entry.entryBox.backgroundColor = ENTRY_FOCUS_BG;
  hintsText.content = HINTS_HISTORY;
  textarea.blur();
}

export function returnFocusToTextarea(): void {
  if (focusedEntry) {
    focusedEntry.entryBox.backgroundColor = undefined;
    setFocusedEntry(null);
  }
  hintsText.content = hintsContent(appMode);
  textarea.focus();
}

// ---------------------------------------------------------------------------
// History entry factory
// ---------------------------------------------------------------------------

export function addHistoryEntry(source: string): EntryState {
  const entryBox = new BoxRenderable(renderer, {
    flexDirection: "column",
    paddingX: 2,
    paddingTop: 1,
    paddingBottom: 1,
  });

  const sourceText = new TextRenderable(renderer, {
    content: source,
    attributes: TextAttributes.DIM,
    wrapMode: "word",
    width: "100%",
  });

  const resultText = new TextRenderable(renderer, {
    content: "Translating…",
    fg: "#00d7d7",
    attributes: TextAttributes.DIM,
    wrapMode: "word",
    width: "100%",
  });

  // Forward ref so the closure captures the final EntryState object.
  const entryRef: { entry: EntryState | null } = { entry: null };
  const handleClick = () => {
    if (entryRef.entry) focusHistoryEntry(entryRef.entry);
  };

  entryBox.onMouseDown   = handleClick;
  sourceText.onMouseDown = handleClick;
  resultText.onMouseDown = handleClick;

  entryBox.add(sourceText);
  entryBox.add(resultText);
  historyBox.add(entryBox);

  const entry: EntryState = {
    entryBox,
    sourceText,
    resultText,
    originalSource: source,
    translationText: "",
  };
  entryRef.entry = entry;
  entries.push(entry);

  return entry;
}

// ---------------------------------------------------------------------------
// Delete helper
// ---------------------------------------------------------------------------

export function deleteEntry(entry: EntryState): void {
  const idx = entries.indexOf(entry);
  if (idx === -1) return;
  entries.splice(idx, 1);
  historyBox.remove(entry.entryBox.id);

  // Move keyboard focus to the nearest remaining entry.
  if (focusedEntry === entry) {
    setFocusedEntry(null);
    if (entries.length === 0) {
      returnFocusToTextarea();
    } else {
      focusHistoryEntry(entries[Math.min(idx, entries.length - 1)]!);
    }
  }
}
