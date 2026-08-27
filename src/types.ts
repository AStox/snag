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

export type Provider = "openai" | "anthropic" | "xai";

export type ModelOption = { id: string; label: string };

export const PROVIDER_META: Record<
  Provider,
  {
    label: string;
    defaultModel: string;
    keyPlaceholder: string;
    consoleUrl: string;
    models: ModelOption[];
  }
> = {
  openai: {
    label: "OpenAI",
    defaultModel: "gpt-4o",
    keyPlaceholder: "sk-…",
    consoleUrl: "https://platform.openai.com/api-keys",
    models: [
      { id: "gpt-4o", label: "GPT-4o" },
      { id: "gpt-4o-mini", label: "GPT-4o mini" },
      { id: "gpt-4.1", label: "GPT-4.1" },
      { id: "gpt-4.1-mini", label: "GPT-4.1 mini" },
    ],
  },
  anthropic: {
    label: "Anthropic",
    defaultModel: "claude-sonnet-4-5",
    keyPlaceholder: "sk-ant-…",
    consoleUrl: "https://console.anthropic.com/settings/keys",
    models: [
      { id: "claude-sonnet-4-5", label: "Sonnet 4.5" },
      { id: "claude-opus-4-1", label: "Opus 4.1" },
      { id: "claude-haiku-4-5", label: "Haiku 4.5" },
    ],
  },
  xai: {
    label: "xAI",
    defaultModel: "grok-4-fast-non-reasoning",
    keyPlaceholder: "xai-…",
    consoleUrl: "https://console.x.ai/team/default/api-keys",
    models: [
      { id: "grok-4-fast-non-reasoning", label: "Grok 4 Fast" },
      { id: "grok-4-fast-reasoning", label: "Grok 4 Fast (reasoning)" },
      { id: "grok-4.6", label: "Grok 4.6" },
    ],
  },
};

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
