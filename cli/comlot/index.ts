import { createCliRenderer } from "@opentui/core";
import { createRoot } from "@opentui/react";
import { App } from "./app";

const app = App();

const renderer = await createCliRenderer();
createRoot(renderer).render(app);  