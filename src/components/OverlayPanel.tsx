import type { SessionState } from "../types";

const COPY: Record<string, { title: string; hint: string }> = {
  listening: { title: "Reading the screen…", hint: "Whatever is under the cursor." },
  processing: { title: "Reading the screen…", hint: "Whatever is under the cursor." },
  done: { title: "Nothing to snag", hint: "" },
  error: { title: "Couldn’t snag that", hint: "Try again, or check Settings." },
  explain: { title: "Snag needs Screen Recording and Accessibility", hint: "Screen Recording and Accessibility so Grain-length docs can be read. Microphone is optional." },
};

export function OverlayPanel({
  session,
  onCancel,
}: {
  session: SessionState;
  onCancel?: () => void;
}) {
  if (session.phase === "idle") return null;
  const copy = COPY[session.phase] ?? COPY.processing;
  const shownTitle =
    session.phase === "done" ? session.title || "Nothing to snag" : copy.title;
  const shownHint =
    session.phase === "done"
      ? session.hint || ""
      : session.hint || copy.hint;
  const canCancel = session.phase === "listening" || session.phase === "processing";

  return (
    <div className="pill-card" role="status">
      <div className={`orb ${session.phase}`}>
        {session.phase === "listening" && <span className="ring" />}
        <span className="orb-dot" />
      </div>
      <div className="ov-copy">
        <strong>{shownTitle}</strong>
        {shownHint ? <span>{shownHint}</span> : null}
      </div>
      {canCancel && (
        <button className="ov-esc" onClick={onCancel} type="button">
          esc
        </button>
      )}
    </div>
  );
}
