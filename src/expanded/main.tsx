import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { ToastApp } from "../toast/ToastApp";
import "../shared/base.css";
import "../toast/styles.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ToastApp mode="expanded" />
  </StrictMode>,
);
