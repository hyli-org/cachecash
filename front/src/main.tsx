// buffer-polyfill MUST be the very first import so that globalThis.Buffer is
// available before @aztec/bb.js evaluates its static class initialisers.
import "./buffer-polyfill";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import App from "./App.tsx";

createRoot(document.getElementById("root")!).render(
    <StrictMode>
        <App />
    </StrictMode>
);
