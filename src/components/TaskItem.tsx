import { useState } from "react";
import type { Task, TaskStatus } from "../types";
import { relativeTime } from "../lib/backend";

export function TaskItem({
  task,
  onPatch,
  onDelete,
}: {
  task: Task;
  onPatch: (id: string, patch: Partial<Task>) => void;
  onDelete: (id: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [editingNotes, setEditingNotes] = useState(false);

  const cycle: TaskStatus[] = ["inbox", "doing", "done"];
  const nextStatus = cycle[(cycle.indexOf(task.status) + 1) % cycle.length];

  return (
    <article className="task" data-status={task.status}>
      <button
        className="check"
        title={task.status === "done" ? "Reopen" : "Complete"}
        onClick={() =>
          onPatch(task.id, {
            status: task.status === "done" ? "inbox" : "done",
          })
        }
      />
      <div>
        <div
          className="title"
          contentEditable
          suppressContentEditableWarning
          onBlur={(e) => {
            const title = e.currentTarget.textContent?.trim() || task.title;
            if (title !== task.title) onPatch(task.id, { title });
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              (e.currentTarget as HTMLElement).blur();
            }
          }}
        >
          {task.title}
        </div>
        <div className="meta">
          {task.sourceApp && <span className="pill">{task.sourceApp}</span>}
          {task.dueHint && <span>Due {task.dueHint}</span>}
          <span>{relativeTime(task.createdAt)}</span>
          {task.notes && (
            <button className="ghost" onClick={() => setOpen(!open)}>
              {open ? "Hide notes" : "Notes"}
            </button>
          )}
        </div>
        {open && !editingNotes && (
          <p className="notes" onDoubleClick={() => setEditingNotes(true)}>
            {task.notes || "No notes"}
          </p>
        )}
        {editingNotes && (
          <textarea
            className="notes-edit"
            defaultValue={task.notes}
            autoFocus
            onBlur={(e) => {
              onPatch(task.id, { notes: e.currentTarget.value });
              setEditingNotes(false);
              setOpen(true);
            }}
          />
        )}
      </div>
      <div className="actions">
        {task.status !== "done" && (
          <button className="ghost" onClick={() => onPatch(task.id, { status: nextStatus })}>
            {task.status === "inbox" ? "Doing" : "Inbox"}
          </button>
        )}
        <button className="ghost" onClick={() => { setOpen(true); setEditingNotes(true); }}>
          Edit
        </button>
        <button className="ghost danger" onClick={() => onDelete(task.id)}>
          Delete
        </button>
      </div>
    </article>
  );
}
