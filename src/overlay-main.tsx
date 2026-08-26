import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { OverlayPanel } from "./components/OverlayPanel";
import { getBackend } from "./lib/backend";
import type { SessionState } from "./types";
import "./index.css";

function OverlayApp() {
  const [session, setSession] = useState<SessionState>({ phase: "idle" });

  useEffect(() => {
    let un: (() => void) | undefined;
    (async () => {
      const b = await getBackend();
      un = await b.onSession(setSession);
    })();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        void getBackend().then((b) => b.cancelCapture());
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      un?.();
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  return (
    <div className="overlay-root">
      <OverlayPanel
        session={session.phase === "idle" ? { phase: "listening" } : session}
        onCancel={() => void getBackend().then((b) => b.cancelCapture())}
      />
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <OverlayApp />
  </React.StrictMode>,
);
