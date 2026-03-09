#!/usr/bin/env bun

import { createCliRenderer } from "@opentui/core";
import {
  createRoot,
  useKeyboard,
  useRenderer,
  useTerminalDimensions,
} from "@opentui/react";

import { ServiceList } from "./componenets/service-list";
import { LogViewer } from "./componenets/log-viewer";
import { useMemo } from "react";
import { AppProvider, FocusedProvider } from "./context/app-state";

export function App() {
  const { width, height } = useTerminalDimensions();
  const renderer = useRenderer();

  useKeyboard((event) => {
    if (event.name === "q") {
      renderer.destroy();
      process.exit(0);
    }
    if (event.name === "p") {
      renderer.console.toggle();
    }
  });

  return (
    <AppProvider>
      <FocusedProvider>
        <box style={{ width, height, flexDirection: "column" }}>
          <box
            style={{
              paddingX: 1,
              borderStyle: "rounded",
              borderColor: "cyan",
              height: Math.floor(height / 2),
            }}
          >
            <text style={{ fg: "cyan", marginBottom: 1 }}>
              Comlot - Docker Compose TUI
            </text>
            <ServiceList />
          </box>
          <box
            style={{
              paddingX: 1,
              borderStyle: "rounded",
              borderColor: "cyan",
              height: Math.ceil(height / 2),
            }}
          >
            <LogViewer />
          </box>
        </box>
      </FocusedProvider>
    </AppProvider>
  );
}

if (import.meta.main) {
  const renderer = await createCliRenderer();
  createRoot(renderer).render(<App />);
}
