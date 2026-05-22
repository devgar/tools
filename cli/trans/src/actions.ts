import { TextAttributes } from "@opentui/core";
import type { EntryState } from "./types";
import {
  isTranslating,
  setIsTranslating,
  appMode,
  setAppMode,
  editingEntry,
  setEditingEntry,
  focusedEntry,
  setFocusedEntry,
  dirParts,
  inputTitle,
  hintsContent,
} from "./state";
import { inputBox, textarea, hintsText } from "./layout";
import { runTranslate } from "./translate";
import { addHistoryEntry } from "./history";

// ---------------------------------------------------------------------------
// Edit helpers
// ---------------------------------------------------------------------------

export function startEdit(entry: EntryState): void {
  if (focusedEntry && focusedEntry !== entry) {
    focusedEntry.entryBox.backgroundColor = undefined;
    setFocusedEntry(null);
  }
  setEditingEntry(entry);
  setAppMode("editing");
  textarea.setText(entry.originalSource);
  inputBox.title = inputTitle();
  hintsText.content = hintsContent("editing");
  textarea.focus();
}

export function cancelEdit(): void {
  setEditingEntry(null);
  setAppMode("normal");
  textarea.setText("");
  inputBox.title = inputTitle();
  hintsText.content = hintsContent("normal");
}

// ---------------------------------------------------------------------------
// Translation trigger
// ---------------------------------------------------------------------------

export async function triggerTranslation(): Promise<void> {
  if (isTranslating) return;

  const text = textarea.plainText.trim();
  if (!text) return;

  setIsTranslating(true);
  const [src, tgt] = dirParts();

  const isEdit = appMode === "editing" && editingEntry !== null;
  const targetEntry = editingEntry;

  // Reset edit state before async gap
  if (isEdit) {
    setEditingEntry(null);
    setAppMode("normal");
    inputBox.title = inputTitle();
    hintsText.content = hintsContent("normal");
  }

  textarea.setText("");

  let entry: EntryState;
  if (isEdit && targetEntry) {
    // Update the existing history entry in-place
    entry = targetEntry;
    entry.originalSource = text;
    entry.translationText = "";
    entry.sourceText.content = text;
    entry.resultText.content = "Translating…";
    entry.resultText.attributes = TextAttributes.DIM;
  } else {
    entry = addHistoryEntry(text);
  }

  const translation = await runTranslate(text, src, tgt);

  if (translation) {
    entry.translationText = translation;
    entry.resultText.attributes = TextAttributes.NONE;
    entry.resultText.content = translation;
  } else {
    entry.resultText.content = "(no result)";
  }

  setIsTranslating(false);
  textarea.focus();
}
