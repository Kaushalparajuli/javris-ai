import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import MiniJarvis from "./components/MiniJarvis";
import "./styles.css";

// The same bundle serves both windows; the small one shows only the mini view.
const mini = getCurrentWindow().label === "mini";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{mini ? <MiniJarvis /> : <App />}</React.StrictMode>,
);
