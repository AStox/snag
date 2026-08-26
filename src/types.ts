export type TaskStatus = "inbox" | "doing" | "done";

export type Task = {
  id: string;
  title: string;
  notes: string;
  status: TaskStatus;
  dueHint: string | null;
  sourceApp: string | null;
  sourceWindow: string | null;
  confidence: number | null;
  createdAt: string;
  updatedAt: string;
  completedAt: string | null;
};

export type Provider = "openai" | "anthropic";

export type AppSettings = {
  hotkey: string;
  provider: Provider;
  apiKey: string;
  model: string;
  sendFullScreenshot: boolean;
  demoMode: boolean;
  demoFixture: "slack-thread" | "github-pr" | "auto";
  permissionsExplained: boolean;
};

export type SessionPhase =
  | "idle"
  | "explain"
  | "listening"
  | "processing"
  | "done"
  | "error";

export type SessionState = {
  phase: SessionPhase;
  hint?: string;
  title?: string;
  error?: string;
};

export type PermissionStatus = {
  screen: "granted" | "denied" | "unknown";
  microphone: "granted" | "denied" | "unknown";
  accessibility: "granted" | "denied" | "unknown";
  platform: "macos" | "other";
};

export type ExtractedTask = {
  title: string;
  notes: string;
  dueHint: string | null;
  sourceApp: string | null;
  confidence: number;
  hasTask: boolean;
};

export const DEFAULT_SETTINGS: AppSettings = {
  hotkey: "alt+space",
  provider: "openai",
  apiKey: "",
  model: "gpt-4o",
  sendFullScreenshot: true,
  demoMode: true,
  demoFixture: "auto",
  permissionsExplained: false,
};
