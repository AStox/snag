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

export function isGenericTranscript(transcript: string): boolean {
  const s = transcript.trim().toLowerCase().replace(/[?.!]+$/g, "");
  if (!s) return true;
  if (GENERIC.some((p) => s === p || s.includes(p))) return true;
  return s.split(/\s+/).length <= 4 && GENERIC.some((p) => s.includes(p.split(" ")[0]));
}

export function cleanTitle(raw: string): string {
  let t = raw.trim();
  t = t.replace(/^(please\s+)?(add|create|make|snag|save|remember)\s+(this\s+)?(as\s+a\s+)?(task|todo)?\s*(for me)?[:\-\s]*/i, "");
  t = t.replace(/[.]+$/, "");
  if (!t) return raw.trim();
  return t.charAt(0).toUpperCase() + t.slice(1);
}

export function heuristicExtract(
  transcript: string,
  fixture?: FixtureMeta | null,
  sourceApp?: string | null,
): ExtractedTask {
  const generic = isGenericTranscript(transcript);
  if (generic && fixture) {
    return {
      title: fixture.caption,
      notes: fixture.notesHint,
      dueHint: fixture.dueHint,
      sourceApp: fixture.sourceApp,
      confidence: 0.62,
    };
  }
  if (generic) {
    const app = sourceApp || "the current app";
    return {
      title: `Follow up in ${app}`,
      notes: transcript.trim() ? `Voice: ${transcript.trim()}` : "",
      dueHint: null,
      sourceApp: sourceApp || null,
      confidence: 0.4,
    };
  }
  return {
    title: cleanTitle(transcript),
    notes: fixture
      ? `On screen (${fixture.sourceApp}): ${fixture.notesHint}`
      : sourceApp
        ? `Captured from ${sourceApp}`
        : "",
    dueHint: fixture?.dueHint ?? null,
    sourceApp: fixture?.sourceApp ?? sourceApp ?? null,
    confidence: 0.74,
  };
}
