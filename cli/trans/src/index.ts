import { focusedEntry } from "./state";
import { renderer, inputBox, historyBox, textarea } from "./layout";
import { focusHistoryEntry, returnFocusToTextarea } from "./history";
import { triggerTranslation } from "./actions";
import { dispatchKeypress } from "./keybindings";

// ---------------------------------------------------------------------------
// Textarea submit
// ---------------------------------------------------------------------------

textarea.onSubmit = () => { triggerTranslation(); };

// ---------------------------------------------------------------------------
// Renderer event wiring
// ---------------------------------------------------------------------------

// Restore focus after returning from another application.
renderer.on("focus", () => {
  if (focusedEntry) {
    focusHistoryEntry(focusedEntry);
  } else {
    textarea.focus();
  }
});

// Catch-all mouse handlers to recover focus on background clicks.
historyBox.onMouseDown = () => returnFocusToTextarea();
inputBox.onMouseDown   = () => returnFocusToTextarea();

// ---------------------------------------------------------------------------
// Global keybindings
// ---------------------------------------------------------------------------

renderer.keyInput.on("keypress", (key) => dispatchKeypress(key));

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

textarea.focus();
