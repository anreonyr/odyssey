// React entry point. Vite serves this in dev and bundles it for
// prod; the `index.html` shipped at the project root references
// `/src/main.tsx` directly.

import "@fontsource-variable/jetbrains-mono";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";

import { App } from "./App";

import "./index.css";

const root = document.getElementById("root");
if (!root) throw new Error("missing #root in index.html");

createRoot(root).render(
  <StrictMode>
    <BrowserRouter>
      <App />
    </BrowserRouter>
  </StrictMode>,
);
