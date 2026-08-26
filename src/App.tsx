import { useEffect, useMemo, useState } from "react";
import { OverlayPanel } from "./components/OverlayPanel";
import { SettingsPanel } from "./components/SettingsPanel";
import { TaskItem } from "./components/TaskItem";
import {
  formatHotkey,
  getBackend,
  type Backend,
} from "./lib/backend";
import type {
  AppSettings,
  PermissionStatus,
  SessionState,
  Task,
  TaskStatus,
} from "./types";
import { DEFAULT_SETTINGS } from "./types";

const TABS: { id: TaskStatus | "all"; label: string }[] = [
  { id: "inbox", label: "Inbox" },
  { id: "doing", label: "Doing" },
  { id: "done", label: "Done" },
];

export default function App() {
  const [backend, setBackend] = useState<Backend | null>(null);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [query, setQuery] = useState("");
  const [tab, setTab] = useState<TaskStatus | "all">("inbox");
  const [showSettings, setShowSettings] = useState(false);
  const [session, setSession] = useState<SessionState>({ phase: "idle" });
  const [perms, setPerms] = useState<PermissionStatus | null>(null);
  const [explainOpen, setExplainOpen] = useState(false);

  useEffect(() => {
    let unSession: (() => void) | undefined;
    let unTasks: (() => void) | undefined;
    let alive = true;
    (async () => {
      const b = await getBackend();
      if (!alive) return;
      setBackend(b);
      const [t, s, p] = await Promise.all([
        b.listTasks(),
        b.getSettings(),
        b.checkPermissions(),
      ]);
      setTasks(t);
      setSettings(s);
      setPerms(p);
      unSession = await b.onSession(setSession);
      unTasks = await b.onTasksChanged(async () => {
        setTasks(await b.listTasks());
      });
    })();
    return () => {
      alive = false;
      unSession?.();
      unTasks?.();
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        backend?.cancelCapture();
        setExplainOpen(false);
        return;
      }
      const isHot =
        (e.code === "Space" || e.key === " ") &&
        (e.altKey || e.metaKey) &&
        !e.repeat;
      if (isHot && !showSettings) {
        e.preventDefault();
        void triggerCapture();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [backend, settings, showSettings, session.phase]);

  async function triggerCapture() {
    if (!backend) return;
    if (session.phase === "processing") return;
    if (!settings.demoMode && !settings.permissionsExplained && backend.isNative) {
      setExplainOpen(true);
      setSession({ phase: "explain" });
      return;
    }
    setShowSettings(false);
    await backend.startCapture();
  }

  async function persistSettings(next: AppSettings) {
    setSettings(next);
    await backend?.saveSettings(next);
  }

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return tasks.filter((t) => {
      if (tab !== "all" && t.status !== tab) return false;
      if (!q) return true;
      return (
        t.title.toLowerCase().includes(q) ||
        t.notes.toLowerCase().includes(q) ||
        (t.sourceApp || "").toLowerCase().includes(q)
      );
    });
  }, [tasks, tab, query]);

  const counts = useMemo(() => {
    return {
      inbox: tasks.filter((t) => t.status === "inbox").length,
      doing: tasks.filter((t) => t.status === "doing").length,
      done: tasks.filter((t) => t.status === "done").length,
    };
  }, [tasks]);

  return (
    <div className="app">
      <div className="drag-region" />
      <header className="top">
        <div className="brand">
          <h1>Snag</h1>
          <span>from the screen</span>
        </div>
        <button
          className={`icon-btn ${showSettings ? "active" : ""}`}
          onClick={() => setShowSettings((v) => !v)}
          title="Settings"
          type="button"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9c.3.7.9 1.2 1.5 1.2H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
          </svg>
        </button>
      </header>

      {settings.demoMode && !showSettings && (
        <div className="demo-banner">Demo mode — fixtures, no Screen Recording required</div>
      )}

      {showSettings && backend && (
        <SettingsPanel
          settings={settings}
          perms={perms}
          native={backend.isNative}
          onChange={(s) => void persistSettings(s)}
          onRequestPerms={async () => {
            await backend.acknowledgePermissions();
            const p = await backend.requestPermissions();
            setPerms(p);
            setSettings(await backend.getSettings());
          }}
        />
      )}

      {!showSettings && (
        <>
          <div className="search">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <circle cx="11" cy="11" r="7" />
              <path d="M20 20l-3-3" />
            </svg>
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search tasks"
            />
          </div>
          <div className="tabs">
            {TABS.map((t) => (
              <button
                key={t.id}
                data-on={tab === t.id}
                onClick={() => setTab(t.id)}
                type="button"
              >
                {t.label}
                <span className="count">{counts[t.id as TaskStatus]}</span>
              </button>
            ))}
          </div>
          <div className="list">
            {explainOpen && (
              <div className="explain" style={{ padding: "12px 10px 20px" }}>
                <h3>Before macOS asks</h3>
                <p>
                  Snag will capture the display your cursor is on and read as much on-screen text
                  as it can (a Grain transcript, Slack thread, PR). Screenshots are not saved —
                  only the tasks that come back. You do not need to speak. No per-source APIs.
                </p>
                <p>
                  You’ll see system prompts for Screen Recording and Accessibility. Accessibility
                  is how Grain-length documents get read. Microphone is optional and not required.
                </p>
                <div className="actions-row">
                  <button className="btn" type="button" onClick={() => { setExplainOpen(false); setSession({ phase: "idle" }); }}>
                    Not now
                  </button>
                  <button
                    className="btn primary"
                    type="button"
                    onClick={async () => {
                      if (!backend) return;
                      await backend.acknowledgePermissions();
                      await backend.requestPermissions();
                      setExplainOpen(false);
                      setSettings(await backend.getSettings());
                      await backend.startCapture();
                    }}
                  >
                    Continue
                  </button>
                </div>
              </div>
            )}
            {filtered.length === 0 && !explainOpen && (
              <div className="empty">
                <h2>Nothing here yet</h2>
                <p>
                  Point at a transcript, Slack thread, or PR and press{" "}
                  <span className="kbd">{formatHotkey(settings.hotkey)}</span>. Snag pulls every
                  action item — no Grain, Slack, or GitHub APIs.
                </p>
              </div>
            )}
            {filtered.map((task) => (
              <TaskItem
                key={task.id}
                task={task}
                onPatch={(id, patch) => backend?.updateTask(id, patch).then(() => backend.listTasks()).then(setTasks)}
                onDelete={(id) => backend?.deleteTask(id).then(() => backend.listTasks()).then(setTasks)}
              />
            ))}
          </div>
          <footer className="foot">
            <span>Tasks stay on this machine. Captures do not.</span>
            <button className="snag-btn" type="button" onClick={() => void triggerCapture()}>
              Snag
            </button>
          </footer>
        </>
      )}

      {session.phase !== "idle" && !backend?.isNative && (
        <div className="inline-overlay">
          <OverlayPanel session={session} onCancel={() => backend?.cancelCapture()} />
        </div>
      )}
    </div>
  );
}
