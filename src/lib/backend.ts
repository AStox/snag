import type {
  AppSettings,
  PermissionStatus,
  SessionState,
  Task,
} from "../types";
import { DEFAULT_SETTINGS } from "../types";
import { FIXTURES, FIXTURE_ORDER, type FixtureId } from "./fixtures";
import { heuristicExtract, overlayTitle, shouldFile } from "./heuristic";

export type Unlisten = () => void;

export type Backend = {
  isNative: boolean;
  listTasks: () => Promise<Task[]>;
  upsertTask: (task: Task) => Promise<void>;
  updateTask: (id: string, patch: Partial<Task>) => Promise<Task>;
  deleteTask: (id: string) => Promise<void>;
  getSettings: () => Promise<AppSettings>;
  saveSettings: (settings: AppSettings) => Promise<void>;
  startCapture: () => Promise<void>;
  stopCapture: () => Promise<void>;
  cancelCapture: () => Promise<void>;
  acknowledgePermissions: () => Promise<void>;
  checkPermissions: () => Promise<PermissionStatus>;
  requestPermissions: () => Promise<PermissionStatus>;
  onSession: (fn: (s: SessionState) => void) => Promise<Unlisten>;
  onTasksChanged: (fn: () => void) => Promise<Unlisten>;
};

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function uid(): string {
  return crypto.randomUUID();
}

function nowIso(): string {
  return new Date().toISOString();
}

const TASKS_KEY = "snag.tasks.v1";
const SETTINGS_KEY = "snag.settings.v1";
const SESSION_EVENT = "snag-session";
const TASKS_EVENT = "snag-tasks";

function loadTasks(): Task[] {
  try {
    const raw = localStorage.getItem(TASKS_KEY);
    return raw ? (JSON.parse(raw) as Task[]) : [];
  } catch {
    return [];
  }
}

function saveTasks(tasks: Task[]) {
  localStorage.setItem(TASKS_KEY, JSON.stringify(tasks));
  window.dispatchEvent(new Event(TASKS_EVENT));
}

function loadSettings(): AppSettings {
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    return raw ? { ...DEFAULT_SETTINGS, ...(JSON.parse(raw) as AppSettings) } : { ...DEFAULT_SETTINGS };
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

function saveSettingsLocal(s: AppSettings) {
  const stored = { ...s };
  localStorage.setItem(SETTINGS_KEY, JSON.stringify(stored));
}

let demoIndex = 0;
let demoTimer: number | null = null;
let capturePhase: SessionState["phase"] = "idle";
let sessionListeners: Array<(s: SessionState) => void> = [];

function emitSession(s: SessionState) {
  sessionListeners.forEach((fn) => fn(s));
  window.dispatchEvent(new CustomEvent(SESSION_EVENT, { detail: s }));
}

function pickFixture(settings: AppSettings): FixtureId {
  if (settings.demoFixture !== "auto") return settings.demoFixture;
  const id = FIXTURE_ORDER[demoIndex % FIXTURE_ORDER.length];
  demoIndex += 1;
  return id;
}

function sleep(ms: number) {
  return new Promise((r) => setTimeout(r, ms));
}

async function completeWebCapture() {
  if (capturePhase !== "processing") return;
  emitSession({ phase: "processing", hint: "Whatever is under the cursor." });
  await sleep(280);
  if (capturePhase !== "processing") return;
  const settings = loadSettings();
  const fixture = FIXTURES[pickFixture(settings)];
  const extracted = heuristicExtract("", fixture, fixture.sourceApp, null).filter(shouldFile);
  if (extracted.length === 0) {
    capturePhase = "done";
    emitSession({ phase: "done", title: "Nothing to snag" });
    await sleep(1400);
    if (capturePhase === "done") {
      capturePhase = "idle";
      emitSession({ phase: "idle" });
    }
    return;
  }
  const stamp = nowIso();
  const tasks = loadTasks();
  for (const item of extracted) {
    const task: Task = {
      id: uid(),
      title: item.title,
      notes: item.notes,
      status: "inbox",
      dueHint: item.dueHint,
      sourceApp: item.sourceApp,
      sourceWindow: fixture.windowTitle,
      confidence: item.confidence,
      createdAt: stamp,
      updatedAt: stamp,
      completedAt: null,
    };
    tasks.push(task);
  }
  saveTasks(tasks);
  capturePhase = "done";
  const title = overlayTitle(extracted);
  emitSession({
    phase: "done",
    title,
    hint: title === "Nothing to snag" ? undefined : "Saved locally — screenshot discarded.",
  });
  await sleep(1400);
  if (capturePhase === "done") {
    capturePhase = "idle";
    emitSession({ phase: "idle" });
  }
}

const webBackend: Backend = {
  isNative: false,
  async listTasks() {
    return loadTasks().sort((a, b) => b.createdAt.localeCompare(a.createdAt));
  },
  async upsertTask(task) {
    const tasks = loadTasks().filter((t) => t.id !== task.id);
    tasks.push(task);
    saveTasks(tasks);
  },
  async updateTask(id, patch) {
    const tasks = loadTasks();
    const idx = tasks.findIndex((t) => t.id === id);
    if (idx < 0) throw new Error("Task not found");
    const next = { ...tasks[idx], ...patch, updatedAt: nowIso() };
    if (patch.status === "done" && !next.completedAt) next.completedAt = nowIso();
    if (patch.status && patch.status !== "done") next.completedAt = null;
    tasks[idx] = next;
    saveTasks(tasks);
    return next;
  },
  async deleteTask(id) {
    saveTasks(loadTasks().filter((t) => t.id !== id));
  },
  async getSettings() {
    return loadSettings();
  },
  async saveSettings(settings) {
    saveSettingsLocal(settings);
  },
  async startCapture() {
    if (capturePhase === "processing") return;
    if (demoTimer) {
      window.clearTimeout(demoTimer);
      demoTimer = null;
    }
    capturePhase = "processing";
    emitSession({ phase: "processing", hint: "Whatever is under the cursor." });
    // Short processing beat — no 1.6s listening delay, no voice.
    demoTimer = window.setTimeout(() => {
      demoTimer = null;
      void completeWebCapture();
    }, 400);
  },
  async stopCapture() {
    // Voice/listen-stop is gone from the default flow. Ignore.
  },
  async cancelCapture() {
    if (demoTimer) {
      window.clearTimeout(demoTimer);
      demoTimer = null;
    }
    capturePhase = "idle";
    emitSession({ phase: "idle" });
  },
  async acknowledgePermissions() {
    const s = loadSettings();
    s.permissionsExplained = true;
    saveSettingsLocal(s);
  },
  async checkPermissions() {
    return { screen: "unknown", microphone: "unknown", accessibility: "unknown", platform: "other" };
  },
  async requestPermissions() {
    return webBackend.checkPermissions();
  },
  async onSession(fn) {
    sessionListeners.push(fn);
    const handler = (e: Event) => fn((e as CustomEvent<SessionState>).detail);
    window.addEventListener(SESSION_EVENT, handler);
    return () => {
      sessionListeners = sessionListeners.filter((x) => x !== fn);
      window.removeEventListener(SESSION_EVENT, handler);
    };
  },
  async onTasksChanged(fn) {
    window.addEventListener(TASKS_EVENT, fn);
    return () => window.removeEventListener(TASKS_EVENT, fn);
  },
};

async function nativeBackend(): Promise<Backend> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");

  return {
    isNative: true,
    listTasks: () => invoke<Task[]>("list_tasks"),
    upsertTask: (task) => invoke("upsert_task", { task }),
    updateTask: (id, patch) => invoke<Task>("update_task", { id, patch }),
    deleteTask: (id) => invoke("delete_task", { id }),
    getSettings: () => invoke<AppSettings>("get_settings"),
    saveSettings: (settings) => invoke("save_settings", { settings }),
    startCapture: () => invoke("start_capture"),
    stopCapture: () => invoke("stop_capture"),
    cancelCapture: () => invoke("cancel_capture"),
    acknowledgePermissions: () => invoke("acknowledge_permissions"),
    checkPermissions: () => invoke<PermissionStatus>("check_permissions"),
    requestPermissions: () => invoke<PermissionStatus>("request_permissions"),
    async onSession(fn) {
      const un = await listen<SessionState>("snag://session", (e) => fn(e.payload));
      return () => un();
    },
    async onTasksChanged(fn) {
      const un = await listen("snag://tasks", () => fn());
      return () => un();
    },
  };
}

let cached: Backend | null = null;

export async function getBackend(): Promise<Backend> {
  if (cached) return cached;
  cached = isTauri() ? await nativeBackend() : webBackend;
  return cached;
}

export function formatHotkey(hotkey: string): string {
  return hotkey
    .split("+")
    .map((p) => {
      const k = p.trim().toLowerCase();
      if (k === "alt" || k === "option") return "⌥";
      if (k === "cmd" || k === "meta" || k === "command" || k === "super") return "⌘";
      if (k === "ctrl" || k === "control") return "⌃";
      if (k === "shift") return "⇧";
      if (k === "space") return "Space";
      return p.trim().charAt(0).toUpperCase() + p.trim().slice(1);
    })
    .join(" ");
}

export function relativeTime(iso: string): string {
  const t = new Date(iso).getTime();
  const s = Math.round((Date.now() - t) / 1000);
  if (s < 20) return "just now";
  if (s < 60) return `${s}s`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h}h`;
  const d = Math.round(h / 24);
  if (d < 14) return `${d}d`;
  return new Date(iso).toLocaleDateString();
}
