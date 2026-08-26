# Snag

A local TODO list you fill from *anything on screen*. No Slack, Linear, GitHub, Grain, or Telegram integrations. You point, Snag reads the pixels and as much of the on-screen document as Accessibility will give it.

Feel: one global hotkey, floating overlay, done. Job: the user hit Snag because they think there is work in what they are pointing at — a Grain meeting transcript, a Slack thread, a PR. Extract every action item as a separate inbox task.

Canonical: you are in Grain, cursor on a recap. Press Option-Space. Snag screenshots that display, scrapes the transcript text under the cursor (no Grain API), and files every action item, owner, and due date — or shows **Nothing to snag**. Overlay: **Snagged 3 tasks**, the single title, or **Nothing to snag**.

Same flow for Slack, Zoom/Fathom recaps, GitHub, Linear, Telegram, code, PDFs — anything visible. No per-source APIs.

## Privacy

Tasks are saved locally (SQLite on Mac, or this browser in Vite demo). **Screenshots are never persisted.** They live in memory for the capture, then are discarded. The only network call is optional: if you paste an API key, Snag sends the in-memory crop (and optionally the full display) plus scraped document text to OpenAI or Anthropic. Document text is the source of truth when present; images are layout/context.

## Stack

- UI: React + TypeScript + Vite
- Desktop: Tauri 2
- Native (macOS): CoreGraphics screenshot of the display containing the cursor, CGEvent cursor position, NSWorkspace frontmost app, Accessibility window title and document scrape (AXUIElementCopyElementAtPosition plus parent/children walk), global hotkey, SQLite
- Model: OpenAI and/or Anthropic vision. No key? Heuristic fallback from document text or the fixture caption. Voice is not part of the default flow.

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
2. **Accessibility** — so Snag can read Grain-length documents (transcripts, threads, PRs) at the cursor. Prompted with AXIsProcessTrustedWithOptions. If Accessibility is off, screenshots still go through; the scrape is skipped and does not crash.
3. **Microphone** — optional, not required.

If capture comes back empty, open System Settings → Privacy & Security → Screen Recording (and Accessibility) and enable Snag, then restart the app.

## API key

Settings → provider (OpenAI or Anthropic) → model name → paste a key. It stays on this machine.

- OpenAI default model: `gpt-4o`
- Anthropic default: `claude-sonnet-4-5`

Without a key, Snag still extracts tasks from a long on-screen document (action-like lines: TODO, I'll, can you, follow up, we should) or from the fixture caption. It will not file a junk todo such as **Follow up in Grain**.

The model is asked for `{"tasks":[...]}`. Empty array = nothing to snag. A transcript yields many tasks; a single Slack ask can be one.

Settings also choose **full display + crop** vs **crop only** for what is sent to the model. Document text (truncated to about 24k characters) is sent first. The crop is always ~900px radius around the cursor, clamped to the display. If vision+text fails, Snag falls back to text-only chat.

## Demo mode

On by default anywhere that is not macOS, and toggleable in Settings.

Demo injects a fixture screenshot (fake Slack thread with Adam, or fake GitHub PR #482) and extracts 1-2 tasks from the fixture caption/notes. Overlay, extraction, and the list all run. No Screen Recording, no Accessibility, no mic, no API key required. No voice.

### Vite UI (this Linux box, or UI work without Tauri)

```
npm install
npm run dev
```

Open the URL Vite prints (port 1420). Click **Snag** or press Option/Alt+Space. Inbox should get **Follow up with Adam about the Q3 launch timeline** plus a second task from the thread. Next capture alternates to the GitHub PR fixture.

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
src-tauri/src/capture.rs   macOS screenshot + cursor + app/title + AX document scrape
src-tauri/src/audio.rs     mic via cpal on macOS
src-tauri/src/extract.rs   multi-task JSON (vision+text, text-only fallback, heuristic)
src-tauri/src/db.rs        rusqlite, tasks only
src-tauri/fixtures/        PNG fixtures (also in public/fixtures)
```

Capture pipeline (hotkey):

1. Screenshot the display containing the cursor; record cursor coords; frontmost app + window title.
2. Accessibility scrape at the cursor (AXUIElementCopyElementAtPosition, walk AXParent about 12 times, BFS AXChildren capped at about 80 nodes / 80k chars). Prefer the longest string; long selected text first. Store as `document_text`. If Accessibility is off, skip — still send screenshots.
3. Draw a cursor marker (dot + ring). Produce a ~900px-radius crop **and** keep the full display.
4. Overlay: “Reading the screen…”. No mic. Escape cancels if the overlay is up. A second hotkey during processing is ignored.
5. Vision LLM (document text + crop + optional full display) returns `{"tasks":[{"title","notes","due_hint","source_app","confidence"}]}`.
6. Insert one inbox row per extracted task. Overlay **Snagged 3 tasks**, the single title, or **Nothing to snag**. Image dropped.

## Notable files

- `src/App.tsx` — inbox / doing / done, search, live list
- `src/components/OverlayPanel.tsx` — “Reading the screen…” then the task title(s) or “Nothing to snag”
- `src-tauri/src/lib.rs` — tray, hotkey, overlay window, session
- `src-tauri/Info.plist` — Screen Recording + Accessibility + Microphone usage strings
- `scripts/fixtures/` — HTML used to bake the PNGs

## Out of scope (v1)

Windows/Linux live capture, cloud sync, accounts, calendar, browser extension, meeting bots, auto-capture without a hotkey, source-specific APIs (no Grain SDK, no Slack/GitHub API).

## Toolchain notes

Desktop builds expect **Rust 1.85+** (1.88 is fine). On a Mac, `npx tauri icon src-tauri/icons/icon.png` can generate `.icns` if the bundler asks for it.
