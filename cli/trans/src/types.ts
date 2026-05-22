import type { BoxRenderable, TextRenderable } from "@opentui/core";

export type Direction = "en-es" | "es-en";
export type AppMode = "normal" | "editing";

export interface EntryState {
  entryBox: BoxRenderable;
  sourceText: TextRenderable;
  resultText: TextRenderable;
  originalSource: string;
  translationText: string; // plain string, empty while translating
}
