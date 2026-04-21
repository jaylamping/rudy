//! Operator reminders persisted to JSON next to the audit log.
//!
//! The dashboard's `RemindersCard` is the only consumer today. Mutations
//! (create/update/delete) audit-log through the same `AuditLog` as motor
//! params; the file at `<audit_log_dir>/reminders.json` is the source of
//! truth between restarts.
//!
//! No DB. The list is small (operator-authored) and the UI rewrites the
//! whole file on each mutation; that's well inside any reasonable budget
//! and avoids a SQLite dependency for what is essentially a sticky-note
//! drawer.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use anyhow::{Context, Result};
use chrono::Utc;
use uuid::Uuid;

use crate::types::{Reminder, ReminderInput};

#[derive(Debug)]
pub struct ReminderStore {
    path: PathBuf,
    items: RwLock<Vec<Reminder>>,
}

impl ReminderStore {
    /// Open the store at `path`, creating an empty file if absent.
    /// `path` is typically `<audit_log_dir>/reminders.json`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating reminders parent {}", parent.display()))?;
        }
        let items: Vec<Reminder> = if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            if raw.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?
            }
        } else {
            Vec::new()
        };
        Ok(Self {
            path,
            items: RwLock::new(items),
        })
    }

    pub fn list(&self) -> Vec<Reminder> {
        self.items.read().expect("reminders poisoned").clone()
    }

    pub fn get(&self, id: &str) -> Option<Reminder> {
        self.items
            .read()
            .expect("reminders poisoned")
            .iter()
            .find(|r| r.id == id)
            .cloned()
    }

    pub fn create(&self, input: ReminderInput) -> Result<Reminder> {
        let r = Reminder {
            id: Uuid::new_v4().to_string(),
            text: input.text,
            due_at: input.due_at,
            done: input.done,
            created_ms: Utc::now().timestamp_millis(),
        };
        {
            let mut items = self.items.write().expect("reminders poisoned");
            items.push(r.clone());
        }
        self.persist()?;
        Ok(r)
    }

    /// Returns `Some(updated)` if found, `None` otherwise.
    pub fn update(&self, id: &str, input: ReminderInput) -> Result<Option<Reminder>> {
        let updated = {
            let mut items = self.items.write().expect("reminders poisoned");
            match items.iter_mut().find(|r| r.id == id) {
                None => None,
                Some(r) => {
                    r.text = input.text;
                    r.due_at = input.due_at;
                    r.done = input.done;
                    Some(r.clone())
                }
            }
        };
        if updated.is_some() {
            self.persist()?;
        }
        Ok(updated)
    }

    /// Returns `true` if removed.
    pub fn delete(&self, id: &str) -> Result<bool> {
        let removed = {
            let mut items = self.items.write().expect("reminders poisoned");
            let before = items.len();
            items.retain(|r| r.id != id);
            items.len() != before
        };
        if removed {
            self.persist()?;
        }
        Ok(removed)
    }

    fn persist(&self) -> Result<()> {
        let snapshot = self.items.read().expect("reminders poisoned").clone();
        let body = serde_json::to_vec_pretty(&snapshot).context("serializing reminders")?;
        // Atomic-ish: write to <path>.tmp then rename. Avoids a torn file
        // if cortex is killed mid-write.
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, body).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("renaming {} -> {}", tmp.display(), self.path.display()))?;
        Ok(())
    }
}
