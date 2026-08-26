import type { ExtractedTask } from "../types";
import type { FixtureMeta } from "./fixtures";

const GENERIC = [
  "add this as a task for me",
  "add this as a task",
  "add this",
  "snag this",
  "save this",
  "capture this",
  "remember this",
  "make this a task",
  "todo this",
  "remind me about this",
  "turn this into a task",
];

const ACTION_MARKERS = [
  "todo",
  "action:",
  "action item",
  "i'll",
  "i will",
  "can you",
  "follow up",
  "we should",
  "please ",
  "need to",
  "needs to",
  "assigned to",
  "[ ]",
  "- [ ]",
  "will you",
  "let's",
  "lets ",
  "make sure",
  "owner:",
];

export function isGenericTranscript(transcript: string): boolean {
  const s = transcript.trim().toLowerCase().replace(/[?.!]+$/g, "");
  if (!s) return true;
  if (GENERIC.some((p) => s === p || s.includes(p))) return true;
  return s.split(/\s+/).length <= 4 && GENERIC.some((p) => s.includes(p.split(" ")[0]));
}

export function cleanTitle(raw: string): string {
  let t = raw.trim().replace(/^[-*•–—]\s*/, "");
  t = t.replace(/^(please\s+)?(add|create|make|snag|save|remember)\s+(this\s+)?(as\s+a\s+)?(task|todo)?\s*(for me)?[:\-\s]*/i, "");
  t = t.replace(/^(todo|action item|action)\s*[:\-]\s*/i, "");
  t = t.replace(/^(\[\s*\]|- \[\s*\])\s*/i, "");
  t = t.replace(/[.]+$/, "");
  if (!t) return raw.trim();
  return t.charAt(0).toUpperCase() + t.slice(1);
}

export function looksLikeAction(line: string): boolean {
  const l = line.trim().toLowerCase();
  if (l.length < 8) return false;
  return ACTION_MARKERS.some((m) => l.includes(m));
}

function isLongDocument(doc: string): boolean {
  return doc.length > 400 || doc.split(/\n/).filter((l) => l.trim()).length > 6;
}

function shouldFileTitle(title: string): boolean {
  const t = title.trim();
  if (!t) return false;
  const lower = t.toLowerCase();
  if (lower === "untitled" || lower === "nothing to snag" || lower === "n/a" || lower === "none") {
    return false;
  }
  if (lower.startsWith("follow up in ")) {
    const rest = lower.slice("follow up in ".length).trim();
    if (rest && !rest.includes(" ") && rest.length < 24) return false;
  }
  return true;
}

function makeTask(
  title: string,
  notes: string,
  dueHint: string | null,
  sourceApp: string | null,
  confidence: number,
): ExtractedTask {
  return { title, notes, dueHint, sourceApp, confidence, hasTask: true };
}

function fixtureTasks(fixture: FixtureMeta): ExtractedTask[] {
  const out: ExtractedTask[] = [
    makeTask(fixture.caption, fixture.notesHint, fixture.dueHint, fixture.sourceApp, 0.62),
  ];
  if (fixture.extraCaption) {
    out.push(makeTask(fixture.extraCaption, fixture.notesHint, fixture.dueHint, fixture.sourceApp, 0.55));
  }
  return out;
}

function splitActionLines(doc: string, sourceApp?: string | null): ExtractedTask[] {
  const seen = new Set<string>();
  const tasks: ExtractedTask[] = [];
  for (const raw of doc.split(/\n/)) {
    const line = raw.trim();
    if (line.length < 8 || !looksLikeAction(line)) continue;
    const title = cleanTitle(line);
    if (!shouldFileTitle(title)) continue;
    const key = title.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    tasks.push(
      makeTask(
        title,
        `From on-screen document:\n${line.slice(0, 400)}`,
        null,
        sourceApp ?? null,
        0.55,
      ),
    );
    if (tasks.length >= 24) break;
  }
  return tasks;
}

export function heuristicExtract(
  transcript: string,
  fixture?: FixtureMeta | null,
  sourceApp?: string | null,
  documentText?: string | null,
): ExtractedTask[] {
  const doc = documentText?.trim() ?? "";
  if (doc) {
    if (isLongDocument(doc)) {
      const split = splitActionLines(doc, sourceApp ?? fixture?.sourceApp);
      if (split.length) return split;
      // Long doc, no action-like lines: do not invent (never "Follow up in Grain").
      if (!fixture) return [];
    } else if (looksLikeAction(doc) || doc.split(/\n/).some(looksLikeAction)) {
      const split = splitActionLines(doc, sourceApp ?? fixture?.sourceApp);
      if (split.length) return split;
      const title = cleanTitle(doc.split(/\n/).find((l) => l.trim()) || doc);
      if (shouldFileTitle(title)) {
        return [makeTask(title, doc.slice(0, 800), null, sourceApp ?? fixture?.sourceApp ?? null, 0.58)];
      }
    }
  }

  if (fixture) return fixtureTasks(fixture);

  if (!isGenericTranscript(transcript)) {
    return [
      makeTask(
        cleanTitle(transcript),
        sourceApp ? `Captured from ${sourceApp}` : "",
        null,
        sourceApp ?? null,
        0.74,
      ),
    ];
  }

  return [];
}

export function shouldFile(task: ExtractedTask): boolean {
  if (!task.hasTask) return false;
  return shouldFileTitle(task.title);
}

export function overlayTitle(tasks: ExtractedTask[]): string {
  if (tasks.length === 0) return "Nothing to snag";
  if (tasks.length === 1) return tasks[0].title;
  return `Snagged ${tasks.length} tasks`;
}
