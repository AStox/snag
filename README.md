# Snag

A local TODO list you fill from *anything on screen*. No Slack, Linear, GitHub, or Telegram integrations. You point, you speak, Snag reads the pixels.

Feel: Wispr Flow (global hotkey, floating overlay, speak, done). Job: turn whatever the cursor is over into a task.

Canonical: you are in Slack, cursor on a thread with Adam. Press ⌥ Space, say “add this as a task for me”. Snag screenshots that display, marks the cursor, transcribes you, and files **Follow up with Adam about [the actual topic]** plus notes.

Same flow for Linear, GitHub, Zoom, Telegram, code, PDFs — anything visible.

## Privacy

Tasks are saved locally (SQLite on Mac, or this browser in Vite demo). **Screenshots and microphone audio are never persisted.** They live in memory for the capture, then are discarded. The only network call is optional: if you paste an API key, Snag sends the in-memory crop (and optionally the full display) plus the transcript to OpenAI or Anthropic.

## Stack

- UI: React + TypeScript + Vite
- Desktop: Tauri 2
- Native (macOS): CoreGraphics screenshot of the display containing the cursor, CGEvent cursor position, NSWorkspace frontmost app, Accessibility window title, cpal/CoreAudio mic, global hotkey, SQLite
- Model: OpenAI (Whisper + vision) and/or Anthropic vision. No key? Heuristic fallback from the transcript + on-screen caption.

## Mac install

Needs a recent macOS (12+), Xcode CLT, Rust (`rustup`), and Node 20+.

```
npm install
npm run tauri:dev
```

Release:

```
npm run tauri:build
```

The app lives in the menu bar and as a small inbox window. Default hotkey is **Option+Space** (configurable in Settings).

To put this on GitHub yourself:

```
git init
git add .
git commit -m "Snag v1"
git remote add origin https://github.com/AStox/snag.git
git push -u origin main
```

Do not commit `.env` or API keys. The key is stored in local app settings only.

## Permissions

The first *real* capture (demo mode off) shows an in-app explainer **before** macOS prompts. Then:

1. **Screen Recording** — so Snag can see the display under your cursor.
2. **Microphone** — so it can hear a short voice command.

If capture comes back empty, open System Settings → Privacy & Security → Screen Recording and enable Snag, then restart the app.

## API key

Settings → provider (OpenAI or Anthropic) → model name → paste a key. It stays on this machine.

- OpenAI default model: `gpt-4o` (Whisper for speech)
- Anthropic default: `claude-sonnet-4-5`

Without a key, Snag still creates a task: it heuristically parses the transcript and any fixture/on-screen caption. Generic voice (“add this”) means the screen is the content.

Settings also choose **full display + crop** vs **crop only** for what is sent to the model. The crop is always ~900px radius around the cursor, clamped to the display.

## Demo mode

On by default anywhere that is not macOS, and toggleable in Settings.

Demo injects a fixture screenshot (fake Slack thread with Adam, or fake GitHub PR #482) and the transcript **“add this as a task for me”**. Overlay, extraction, and the list all run. No Screen Recording, no mic, no API key required.

### Vite UI (this Linux box, or UI work without Tauri)

```
npm install
npm run dev
```

Open the URL Vite prints (port 1420). Click **Snag** or press Option/Alt+Space. A task titled **Follow up with Adam about the Q3 launch timeline** should land in Inbox. Next capture alternates to the GitHub PR fixture.

```
npm run build
```

builds the web UI.

### Desktop demo (Mac)

Keep **Demo mode** on in Settings. Hotkey or tray → Snag from screen. Same fixtures, same heuristic (or a vision model if a key is set).

## Architecture

```
src/                  React list, overlay, settings
src/lib/backend.ts    Tauri invoke *or* localStorage demo
src-tauri/src/capture.rs   macOS CGDisplayCreateImage + cursor + app/title
src-tauri/src/audio.rs     mic via cpal on macOS
src-tauri/src/extract.rs   Whisper + vision JSON, heuristic fallback
src-tauri/src/db.rs        rusqlite, tasks only
src-tauri/fixtures/        PNG fixtures (also in public/fixtures)
```

Capture pipeline (hotkey):

1. Screenshot the display containing the cursor; record cursor coords; frontmost app + window title.
2. Draw a cursor marker (dot + ring). Produce a ~900px-radius crop **and** keep the full display.
3. Start the mic. Stop on pause, second hotkey, or Escape to cancel.
4. Transcribe. Vision LLM returns `title`, `notes`, optional due hint, `source_app`, `confidence`.
5. Save the task. Overlay confirms the title. List updates. Image and audio dropped.

## Notable files

- `src/App.tsx` — inbox / doing / done, search, live list
- `src/components/OverlayPanel.tsx` — recording pulse + “figuring it out”
- `src-tauri/src/lib.rs` — tray, hotkey, overlay window, session
- `src-tauri/Info.plist` — Screen Recording + Microphone usage strings
- `scripts/fixtures/` — HTML used to bake the PNGs

## Out of scope (v1)

Windows/Linux live capture, cloud sync, accounts, calendar, browser extension, meeting bots, auto-capture without a hotkey, source-specific APIs.

## Toolchain notes

Desktop builds expect **Rust 1.85+** (1.88 is fine). On a Mac, `npx tauri icon src-tauri/icons/icon.png` can generate `.icns` if the bundler asks for it.
