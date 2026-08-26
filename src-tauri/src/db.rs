use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::Result;
use crate::models::{AppSettings, Provider, Task, TaskPatch, TaskStatus};

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                notes TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'inbox',
                due_hint TEXT,
                source_app TEXT,
                source_window TEXT,
                confidence REAL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                completed_at TEXT
            );
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            ",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn list_tasks(&self) -> Result<Vec<Task>> {
        let conn = self.conn.lock().expect("db lock");
        let mut stmt = conn.prepare(
            "SELECT id, title, notes, status, due_hint, source_app, source_window, confidence, created_at, updated_at, completed_at
             FROM tasks ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Task {
                id: row.get(0)?,
                title: row.get(1)?,
                notes: row.get(2)?,
                status: TaskStatus::parse(&row.get::<_, String>(3)?),
                due_hint: row.get(4)?,
                source_app: row.get(5)?,
                source_window: row.get(6)?,
                confidence: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
                completed_at: row.get(10)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn insert_task(&self, task: &Task) -> Result<()> {
        let conn = self.conn.lock().expect("db lock");
        conn.execute(
            "INSERT INTO tasks (id, title, notes, status, due_hint, source_app, source_window, confidence, created_at, updated_at, completed_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                task.id,
                task.title,
                task.notes,
                task.status.as_str(),
                task.due_hint,
                task.source_app,
                task.source_window,
                task.confidence,
                task.created_at,
                task.updated_at,
                task.completed_at
            ],
        )?;
        Ok(())
    }

    pub fn update_task(&self, id: &str, patch: TaskPatch) -> Result<Task> {
        let mut tasks = self.list_tasks()?;
        let task = tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| crate::error::SnagError::from("Task not found"))?;
        if let Some(title) = patch.title {
            task.title = title;
        }
        if let Some(notes) = patch.notes {
            task.notes = notes;
        }
        if let Some(status) = patch.status {
            task.status = status.clone();
            if status == TaskStatus::Done {
                if task.completed_at.is_none() {
                    task.completed_at = Some(chrono::Utc::now().to_rfc3339());
                }
            } else {
                task.completed_at = None;
            }
        }
        if let Some(due) = patch.due_hint {
            task.due_hint = due;
        }
        task.updated_at = chrono::Utc::now().to_rfc3339();
        let updated = task.clone();
        let conn = self.conn.lock().expect("db lock");
        conn.execute(
            "UPDATE tasks SET title=?1, notes=?2, status=?3, due_hint=?4, updated_at=?5, completed_at=?6 WHERE id=?7",
            params![
                updated.title,
                updated.notes,
                updated.status.as_str(),
                updated.due_hint,
                updated.updated_at,
                updated.completed_at,
                updated.id
            ],
        )?;
        Ok(updated)
    }

    pub fn delete_task(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().expect("db lock");
        conn.execute("DELETE FROM tasks WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn get_settings(&self) -> Result<AppSettings> {
        let conn = self.conn.lock().expect("db lock");
        let raw: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key='app'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(raw) = raw {
            let mut s: AppSettings = serde_json::from_str(&raw)?;
            if s.hotkey.is_empty() {
                s.hotkey = "alt+space".into();
            }
            Ok(s)
        } else {
            Ok(AppSettings::default())
        }
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<()> {
        let conn = self.conn.lock().expect("db lock");
        conn.execute(
            "INSERT INTO settings(key, value) VALUES('app', ?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![serde_json::to_string(settings)?],
        )?;
        let _ = settings.provider.as_str();
        let _ = Provider::Openai;
        Ok(())
    }
}
