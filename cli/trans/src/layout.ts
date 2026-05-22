import {
  BoxRenderable,
  TextRenderable,
  TextareaRenderable,
  ScrollBoxRenderable,
  createCliRenderer,
  TextAttributes,
} from "@opentui/core";
import { dirColors, hintsContent, inputTitle } from "./state";

// ---------------------------------------------------------------------------
// Renderer (top-level await — Bun ESM module singleton)
// ---------------------------------------------------------------------------

export const renderer = await createCliRenderer({
  exitOnCtrlC: true,
  useMouse: true,
});

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

// Root: full-height column
export const appBox = new BoxRenderable(renderer, {
  flexDirection: "column",
  flexGrow: 1,
});

// ── Chat-style history area ──────────────────────────────────────────────────
export const historyBox = new ScrollBoxRenderable(renderer, {
  flexGrow: 1,
  stickyScroll: true,
  stickyStart: "bottom",
  scrollY: true,
  scrollX: false,
  contentOptions: { justifyContent: "flex-end" },
});

// ── Input box ────────────────────────────────────────────────────────────────
export const inputBox = new BoxRenderable(renderer, {
  height: 7,
  border: true,
  title: inputTitle(),
  borderColor: dirColors().border,
  focusedBorderColor: dirColors().focusedBorder,
  flexDirection: "column",
});

export const textarea = new TextareaRenderable(renderer, {
  flexGrow: 1,
  wrapMode: "word",
  placeholder: "Type text to translate…",
  placeholderColor: "#555555",
  keyBindings: [
    { name: "return",   action: "submit"  },
    { name: "linefeed", action: "submit"  },
    { name: "return",   shift: true, action: "newline" },
  ],
  // onSubmit wired in index.ts after triggerTranslation is defined
});

inputBox.add(textarea);

// ── Hints bar ────────────────────────────────────────────────────────────────
export const hintsBox = new BoxRenderable(renderer, {
  height: 1,
  paddingX: 1,
});

export const hintsText = new TextRenderable(renderer, {
  content: hintsContent("normal"),
  attributes: TextAttributes.DIM,
});

hintsBox.add(hintsText);

// ── Assemble tree ────────────────────────────────────────────────────────────
appBox.add(historyBox);
appBox.add(inputBox);
appBox.add(hintsBox);
renderer.root.add(appBox);

// ScrollBoxRenderable._focusable defaults to true, which causes autoFocus to
// steal focus from our keyboard-focused entries on every click inside the
// history area. Disable it so the scroll box never participates in focus.
historyBox.focusable = false;

// ---------------------------------------------------------------------------
// Direction color helper (needs access to inputBox)
// ---------------------------------------------------------------------------

export function applyDirColors(): void {
  const { border, focusedBorder } = dirColors();
  inputBox.borderColor = border;
  inputBox.focusedBorderColor = focusedBorder;
}
