import type { KeyEvent } from "@opentui/core";
import {
  direction,
  setDirection,
  appMode,
  focusedEntry,
  editingEntry,
  setEditingEntry,
  setAppMode,
  inputTitle,
  hintsContent,
  dirParts,
} from "./state";
import {
  renderer,
  inputBox,
  textarea,
  applyDirColors,
} from "./layout";
import { copyToClipboard } from "./clipboard";
import { entries, focusHistoryEntry, returnFocusToTextarea, deleteEntry } from "./history";
import { startEdit, cancelEdit, triggerTranslation } from "./actions";

// ---------------------------------------------------------------------------
// Binding type
// ---------------------------------------------------------------------------

type KeyBinding = {
  match: (key: KeyEvent) => boolean;
  handle: (key: KeyEvent) => void;
};

// ---------------------------------------------------------------------------
// History-mode bindings  (active when an entry has keyboard focus)
// ---------------------------------------------------------------------------

const historyBindings: KeyBinding[] = [
  {
    match: (key) => key.name === "up" && !key.ctrl && !key.meta,
    handle: () => {
      const idx = entries.indexOf(focusedEntry!);
      if (idx > 0) focusHistoryEntry(entries[idx - 1]!);
    },
  },
  {
    match: (key) => key.name === "down" && !key.ctrl && !key.meta,
    handle: () => {
      const idx = entries.indexOf(focusedEntry!);
      if (idx < entries.length - 1) {
        focusHistoryEntry(entries[idx + 1]!);
      } else {
        returnFocusToTextarea();
      }
    },
  },
  {
    match: (key) => key.name === "e" && !key.ctrl && !key.meta && !key.shift,
    handle: () => {
      const target = focusedEntry!;
      returnFocusToTextarea();
      startEdit(target);
    },
  },
  {
    match: (key) => key.name === "d" && !key.ctrl && !key.meta && !key.shift,
    handle: () => deleteEntry(focusedEntry!),
  },
  {
    match: (key) => key.name === "c" && !key.ctrl && !key.meta && !key.shift,
    handle: () => {
      if (focusedEntry!.translationText) copyToClipboard(focusedEntry!.translationText);
    },
  },
  {
    match: (key) => key.name === "c" && !key.ctrl && !key.meta && key.shift,
    handle: () => copyToClipboard(focusedEntry!.originalSource),
  },
  {
    match: (key) => key.name === "escape",
    handle: () => returnFocusToTextarea(),
  },
];

// ---------------------------------------------------------------------------
// Normal/editing-mode bindings  (active when textarea has focus)
// ---------------------------------------------------------------------------

const normalBindings: KeyBinding[] = [
  {
    // Escape: three-branch handler — cancel edit → clear textarea → quit
    match: (key) => key.name === "escape",
    handle: () => {
      if (appMode === "editing") {
        cancelEdit();
        return;
      }
      if (textarea.plainText.trim()) {
        textarea.setText("");
        return;
      }
      renderer.destroy();
      process.exit(0);
    },
  },
  {
    // UP from textarea first line → enter history navigation
    match: (key) => key.name === "up" && !key.ctrl && !key.meta && !key.shift,
    handle: () => {
      const onFirstLine = !textarea.plainText.slice(0, textarea.cursorOffset).includes("\n");
      if (onFirstLine && entries.length > 0) {
        focusHistoryEntry(entries[entries.length - 1]!);
      }
    },
  },
  {
    match: (key) => key.name === "tab" && !key.ctrl && !key.shift && !key.meta,
    handle: () => {
      setDirection(direction === "en-es" ? "es-en" : "en-es");
      inputBox.title = inputTitle();
      applyDirColors();
    },
  },
  {
    match: (key) => key.ctrl && key.name === "t",
    handle: () => triggerTranslation(),
  },
];

// ---------------------------------------------------------------------------
// Dispatcher
// ---------------------------------------------------------------------------

export function dispatchKeypress(key: KeyEvent): void {
  const bindings = focusedEntry !== null ? historyBindings : normalBindings;

  for (const binding of bindings) {
    if (binding.match(key)) {
      binding.handle(key);
      key.preventDefault();
      return;
    }
  }

  // History catch-all: absorb any unhandled keys to prevent stray characters
  // ending up in the blurred textarea.
  if (focusedEntry !== null) {
    key.preventDefault();
  }
}
