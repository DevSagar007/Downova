import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./ui/tokens.css";
import "./ui/base.css";
import { App } from "./app/App";
import { PreferencesProvider } from "./app/preferences";
import { ServiceProvider } from "./services/ServiceProvider";

const container = document.getElementById("root");
if (!container) throw new Error("#root element missing from index.html");

createRoot(container).render(
  <StrictMode>
    <PreferencesProvider>
      <ServiceProvider>
        <App />
      </ServiceProvider>
    </PreferencesProvider>
  </StrictMode>,
);
