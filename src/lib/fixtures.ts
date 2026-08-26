export type FixtureId = "slack-thread" | "github-pr";

export type FixtureMeta = {
  id: FixtureId;
  sourceApp: string;
  windowTitle: string;
  caption: string;
  notesHint: string;
  dueHint: string | null;
  cursor: { x: number; y: number };
  image: string;
};

export const FIXTURES: Record<FixtureId, FixtureMeta> = {
  "slack-thread": {
    id: "slack-thread",
    sourceApp: "Slack",
    windowTitle: "engineering — Q3 launch",
    caption: "Follow up with Adam about the Q3 launch timeline",
    notesHint:
      "Adam asked whether the mobile cut is still targeting Sept 12, and whether legal review is blocking the help-center copy. Thread in #eng-launch.",
    dueHint: "Sept 12",
    cursor: { x: 560, y: 318 },
    image: "/fixtures/slack-thread.png",
  },
  "github-pr": {
    id: "github-pr",
    sourceApp: "GitHub",
    windowTitle: "PR #482 · cursor-aware screenshot crop",
    caption: "Review PR #482: add cursor-aware screenshot crop",
    notesHint:
      "Maya requested review on feat/snag-crop. Adds a 900px-radius crop around the cursor and draws a marker on the full display capture. Waiting on a look at capture.rs.",
    dueHint: null,
    cursor: { x: 640, y: 280 },
    image: "/fixtures/github-pr.png",
  },
};

export const FIXTURE_ORDER: FixtureId[] = ["slack-thread", "github-pr"];
