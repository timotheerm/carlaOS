import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { QuickMenu } from "./screens/quick";
import "./tokens.css";
import "./app.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {/* The same program serves two windows: the console, and the menu over a game. */}
    {location.hash === "#quick" ? <QuickMenu /> : <App />}
  </StrictMode>,
);
