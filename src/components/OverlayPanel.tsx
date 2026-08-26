import type { SessionState } from "../types";

const COPY: Record<string, { title: string; hint: string }> = {
  listening: { title: "Listening…", hint: "Point at something. Speak, pause, or hit the hotkey again." },
  processing: { title: "Figuring it out…", hint: "Reading the screen. Nothing is being saved yet." },
  done: { title: "Snagged", hint: "Saved locally — screenshot and audio discarded." },
  error: { title: "Couldn’t snag that", hint: "Try again, or check Settings." },
  explain: { title: "Snag needs two permissions", hint: "Screen Recording and Microphone. Continues after you allow." },
};

export function OverlayPanel({
  session,
  onCancel,
}: {
  session: SessionState;
  onCancel?: () => void;
}) {
  if (session.phase === "idle") return null;
  const copy = COPY[session.phase] ?? COPY.listening;
  const shownTitle = session.phase === "done" ? "Snagged" : copy.title;
  const shownHint =
    session.phase === "done"
      ? session.title || copy.hint
      : session.hint || copy.hint;

  return (
    <div className="pill-card" role="status">
      <div className={`orb ${session.phase}`}>
        {session.phase === "listening" && <span className="ring" />}
        <span className="orb-dot" />
      </div>
      <div className="ov-copy">
        <strong>{shownTitle}</strong>
        <span>{shownHint}</span>
      </div>
      {session.phase === "listening" && (
        <button className="ov-esc" onClick={onCancel} type="button">
          esc
        </button>
      )}
    </div>
  );
}
