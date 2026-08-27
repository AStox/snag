import type { AppSettings, PermissionStatus, Provider } from "../types";
import { PROVIDER_META } from "../types";
import { formatHotkey } from "../lib/backend";

function Switch({
  on,
  onToggle,
}: {
  on: boolean;
  onToggle: () => void;
}) {
  return (
    <button className="switch" data-on={on} onClick={onToggle} type="button" aria-pressed={on}>
      <i />
    </button>
  );
}

export function SettingsPanel({
  settings,
  perms,
  native,
  onChange,
  onRequestPerms,
  onOpenConsole,
  onClose,
}: {
  settings: AppSettings;
  perms: PermissionStatus | null;
  native: boolean;
  onChange: (s: AppSettings) => void;
  onRequestPerms: () => void;
  onOpenConsole: (provider: Provider) => void;
  onClose: () => void;
}) {
  const set = (patch: Partial<AppSettings>) => onChange({ ...settings, ...patch });
  const meta = PROVIDER_META[settings.provider];
  const modelIds = meta.models.map((m) => m.id);
  const modelValue = modelIds.includes(settings.model) ? settings.model : meta.defaultModel;

  return (
    <div className="settings">
      <div className="settings-head">
        <h2>Settings</h2>
        <button className="btn" type="button" onClick={onClose}>
          Done
        </button>
      </div>
      <p className="lede">Local only. The key stays on this machine.</p>

      <div className="field">
        <label>Global hotkey</label>
        <input
          value={settings.hotkey}
          onChange={(e) => set({ hotkey: e.target.value.trim().toLowerCase() })}
          placeholder="alt+space"
        />
        <p className="lede" style={{ marginTop: 6 }}>
          Default {formatHotkey("alt+space")}. Use plugin syntax: alt+space, command+shift+s.
        </p>
      </div>

      <div className="toggle">
        <div>
          <strong>Demo mode</strong>
          <p>Inject fixture screenshots. Works without macOS permissions or an API key.</p>
        </div>
        <Switch on={settings.demoMode} onToggle={() => set({ demoMode: !settings.demoMode })} />
      </div>

      {settings.demoMode && (
        <div className="field">
          <label>Demo fixture</label>
          <select
            value={settings.demoFixture}
            onChange={(e) => set({ demoFixture: e.target.value as AppSettings["demoFixture"] })}
          >
            <option value="auto">Alternate Slack / GitHub</option>
            <option value="slack-thread">Slack thread (Adam / Q3 launch)</option>
            <option value="github-pr">GitHub PR #482</option>
          </select>
        </div>
      )}

      <div className="toggle">
        <div>
          <strong>Send full display</strong>
          <p>Always send the cursor crop. Also send the full display unless you want crop-only.</p>
        </div>
        <Switch
          on={settings.sendFullScreenshot}
          onToggle={() => set({ sendFullScreenshot: !settings.sendFullScreenshot })}
        />
      </div>

      <div className="row">
        <div className="field">
          <label>Provider</label>
          <select
            value={settings.provider}
            onChange={(e) => {
              const provider = e.target.value as AppSettings["provider"];
              set({
                provider,
                model: PROVIDER_META[provider].defaultModel,
              });
            }}
          >
            <option value="openai">OpenAI</option>
            <option value="anthropic">Anthropic</option>
            <option value="xai">xAI</option>
          </select>
        </div>
        <div className="field">
          <label>Model</label>
          <select
            value={modelValue}
            onChange={(e) => set({ model: e.target.value })}
          >
            {!modelIds.includes(settings.model) && settings.model ? (
              <option value={settings.model}>{settings.model}</option>
            ) : null}
            {meta.models.map((m) => (
              <option key={m.id} value={m.id}>
                {m.label}
              </option>
            ))}
          </select>
        </div>
      </div>

      <div className="field">
        <label>API key</label>
        <input
          type="password"
          autoComplete="off"
          value={settings.apiKey}
          onChange={(e) => set({ apiKey: e.target.value })}
          placeholder={PROVIDER_META[settings.provider].keyPlaceholder}
        />
        <p className="lede" style={{ marginTop: 8 }}>
          <button
            className="btn"
            type="button"
            onClick={() => onOpenConsole(settings.provider)}
          >
            Get a {PROVIDER_META[settings.provider].label} key
          </button>
        </p>
      </div>

      <div className="privacy">
        <strong>Privacy.</strong> Tasks live in a local SQLite file (or this browser, in Vite demo).
        Screenshots are held in memory for the capture, then discarded.
        Nothing is uploaded except the in-memory image when an API key is set.
      </div>

      <div className="explain">
        <h3>Permissions</h3>
        <p>
          Before macOS prompts, Snag tells you why: Screen Recording to see what’s under the
          cursor, and Accessibility to read Grain-length documents (transcripts, threads, PRs)
          without a Grain/Slack/GitHub API. Microphone is optional and not required. {native ? "This build can request Screen Recording and Accessibility." : "The Vite UI cannot prompt macOS — use the desktop app for that."}
        </p>
        {perms && (
          <p>
            Screen: {perms.screen} · Accessibility: {perms.accessibility} · Microphone: {perms.microphone}
            {perms.platform !== "macos" ? " · not on macOS" : ""}
          </p>
        )}
        <div className="actions-row">
          <button className="btn primary" type="button" onClick={onRequestPerms} disabled={!native}>
            Continue to system prompts
          </button>
        </div>
      </div>
    </div>
  );
}
