import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Overlay from "./Overlay";
import "./i18n";
import { initializeAppearance } from "./appearance";

const isOverlay = new URLSearchParams(window.location.search).get("overlay") === "1";
initializeAppearance();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {isOverlay ? <Overlay /> : <App />}
  </React.StrictMode>,
);
