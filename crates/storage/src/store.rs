use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use chrono::{DateTime, Utc};
use corgigram_crypto::PreKeyBundle;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::StorageError;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContactRecord {
    pub user_id: String,
    pub display_name: String,
    pub bundle: PreKeyBundle,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_data_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutboxRecord {
    pub id: String,
    pub contact_id: String,
    pub body: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MessageRecord {
    pub id: String,
    pub contact_id: String,
    pub direction: String,
    pub body: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    /// text | image | file | album
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_mime: Option<String>,
}

fn default_kind() -> String {
    "text".into()
}

pub struct Storage {
    conn: Mutex<Connection>,
}

impl Storage {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| StorageError::Other(e.to_string()))?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS contacts (
                user_id TEXT PRIMARY KEY,
                display_name TEXT NOT NULL,
                bundle_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                contact_id TEXT NOT NULL,
                direction TEXT NOT NULL,
                body TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                kind TEXT NOT NULL DEFAULT 'text',
                attachment_name TEXT,
                attachment_mime TEXT,
                FOREIGN KEY(contact_id) REFERENCES contacts(user_id)
            );
            CREATE TABLE IF NOT EXISTS outbox (
                id TEXT PRIMARY KEY,
                contact_id TEXT NOT NULL,
                body TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY(contact_id) REFERENCES contacts(user_id)
            );
            ",
        )?;
        let _ = conn.execute(
            "ALTER TABLE messages ADD COLUMN kind TEXT NOT NULL DEFAULT 'text'",
            [],
        );
        let _ = conn.execute("ALTER TABLE messages ADD COLUMN attachment_name TEXT", []);
        let _ = conn.execute("ALTER TABLE messages ADD COLUMN attachment_mime TEXT", []);
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> Result<MutexGuard<'_, Connection>, StorageError> {
        self.conn
            .lock()
            .map_err(|e| StorageError::Other(format!("db lock poisoned: {e}")))
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>, StorageError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), StorageError> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn upsert_contact(&self, contact: &ContactRecord) -> Result<(), StorageError> {
        let bundle_json = serde_json::to_string(&contact.bundle)?;
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO contacts(user_id, display_name, bundle_json, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(user_id) DO UPDATE SET
               display_name = excluded.display_name,
               bundle_json = excluded.bundle_json",
            params![
                contact.user_id,
                contact.display_name,
                bundle_json,
                contact.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_contacts(&self) -> Result<Vec<ContactRecord>, StorageError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT user_id, display_name, bundle_json, created_at FROM contacts ORDER BY display_name",
        )?;
        let rows = stmt.query_map([], |row| {
            let bundle_json: String = row.get(2)?;
            let created_at: String = row.get(3)?;
            Ok(ContactRecord {
                user_id: row.get(0)?,
                display_name: row.get(1)?,
                bundle: serde_json::from_str(&bundle_json).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e))
                })?,
                created_at: created_at.parse().map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
                })?,
                avatar_data_url: None,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
    }

    pub fn get_contact(&self, user_id: &str) -> Result<Option<ContactRecord>, StorageError> {
        Ok(self
            .list_contacts()?
            .into_iter()
            .find(|c| c.user_id == user_id))
    }

    pub fn message_exists(&self, id: &str) -> Result<bool, StorageError> {
        let conn = self.conn()?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn insert_message(&self, message: &MessageRecord) -> Result<(), StorageError> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO messages(id, contact_id, direction, body, status, created_at, kind, attachment_name, attachment_mime)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                message.id,
                message.contact_id,
                message.direction,
                message.body,
                message.status,
                message.created_at.to_rfc3339(),
                message.kind,
                message.attachment_name,
                message.attachment_mime,
            ],
        )?;
        Ok(())
    }

    pub fn upsert_message(&self, message: &MessageRecord) -> Result<(), StorageError> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO messages(id, contact_id, direction, body, status, created_at, kind, attachment_name, attachment_mime)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
               body = excluded.body,
               status = excluded.status,
               kind = excluded.kind,
               attachment_name = excluded.attachment_name,
               attachment_mime = excluded.attachment_mime",
            params![
                message.id,
                message.contact_id,
                message.direction,
                message.body,
                message.status,
                message.created_at.to_rfc3339(),
                message.kind,
                message.attachment_name,
                message.attachment_mime,
            ],
        )?;
        Ok(())
    }

    pub fn update_message_status(&self, id: &str, status: &str) -> Result<(), StorageError> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE messages SET status = ?2 WHERE id = ?1",
            params![id, status],
        )?;
        Ok(())
    }

    pub fn mark_delivered_if_queued(&self, id: &str) -> Result<bool, StorageError> {
        let conn = self.conn()?;
        let changed = conn.execute(
            "UPDATE messages SET status = 'delivered' WHERE id = ?1 AND direction = 'out' AND status = 'queued_mailbox'",
            params![id],
        )?;
        Ok(changed > 0)
    }

    pub fn get_message_status(&self, id: &str) -> Result<Option<String>, StorageError> {
        let conn = self.conn()?;
        let status: Option<String> = conn
            .query_row(
                "SELECT status FROM messages WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .ok();
        Ok(status)
    }

    pub fn list_outbound_by_status(&self, status: &str) -> Result<Vec<MessageRecord>, StorageError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, contact_id, direction, body, status, created_at, kind, attachment_name, attachment_mime
             FROM messages WHERE direction = 'out' AND status = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![status], map_message_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
    }

    pub fn list_messages(&self, contact_id: &str) -> Result<Vec<MessageRecord>, StorageError> {
        self.list_messages_page(contact_id, None, 10_000)
    }

    pub fn list_messages_page(
        &self,
        contact_id: &str,
        before_created_at: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MessageRecord>, StorageError> {
        let conn = self.conn()?;
        let limit = limit.min(500) as i64;
        let rows = if let Some(before) = before_created_at {
            let mut stmt = conn.prepare(
                "SELECT id, contact_id, direction, body, status, created_at, kind, attachment_name, attachment_mime
                 FROM messages WHERE contact_id = ?1 AND created_at < ?2
                 ORDER BY created_at DESC LIMIT ?3",
            )?;
            let mapped = stmt.query_map(params![contact_id, before, limit], map_message_row)?;
            mapped.collect::<Result<Vec<_>, _>>()?
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, contact_id, direction, body, status, created_at, kind, attachment_name, attachment_mime
                 FROM messages WHERE contact_id = ?1
                 ORDER BY created_at DESC LIMIT ?2",
            )?;
            let mapped = stmt.query_map(params![contact_id, limit], map_message_row)?;
            mapped.collect::<Result<Vec<_>, _>>()?
        };
        let mut out = rows;
        out.reverse();
        Ok(out)
    }

    pub fn new_message_id() -> String {
        Uuid::new_v4().to_string()
    }

    pub fn insert_outbox(&self, item: &OutboxRecord) -> Result<(), StorageError> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO outbox(id, contact_id, body, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                item.id,
                item.contact_id,
                item.body,
                item.status,
                item.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_outbox(&self, contact_id: Option<&str>) -> Result<Vec<OutboxRecord>, StorageError> {
        let conn = self.conn()?;
        let (sql, cid) = match contact_id {
            Some(id) => (
                "SELECT id, contact_id, body, status, created_at FROM outbox WHERE contact_id = ?1 ORDER BY created_at ASC",
                Some(id.to_string()),
            ),
            None => (
                "SELECT id, contact_id, body, status, created_at FROM outbox ORDER BY created_at ASC",
                None,
            ),
        };
        let mut stmt = conn.prepare(sql)?;
        let map_row = |row: &rusqlite::Row<'_>| {
            let created_at: String = row.get(4)?;
            Ok(OutboxRecord {
                id: row.get(0)?,
                contact_id: row.get(1)?,
                body: row.get(2)?,
                status: row.get(3)?,
                created_at: created_at.parse().unwrap_or_else(|_| Utc::now()),
            })
        };
        let rows = if let Some(id) = cid {
            stmt.query_map(params![id], map_row)?
        } else {
            stmt.query_map([], map_row)?
        };
        rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
    }

    pub fn delete_outbox(&self, id: &str) -> Result<(), StorageError> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM outbox WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn outbox_count(&self) -> Result<usize, StorageError> {
        let conn = self.conn()?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM outbox", [], |r| r.get(0))?;
        Ok(count as usize)
    }

    fn read_cursor_key(contact_id: &str) -> String {
        format!("read_cursor:{contact_id}")
    }

    pub fn get_read_cursor(&self, contact_id: &str) -> Result<Option<DateTime<Utc>>, StorageError> {
        Ok(self
            .get_meta(&Self::read_cursor_key(contact_id))?
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|d| d.with_timezone(&Utc)))
    }

    pub fn set_read_cursor(&self, contact_id: &str, at: DateTime<Utc>) -> Result<(), StorageError> {
        self.set_meta(&Self::read_cursor_key(contact_id), &at.to_rfc3339())
    }

    pub fn count_unread(&self, contact_id: &str) -> Result<usize, StorageError> {
        let cursor = self.get_read_cursor(contact_id)?;
        let conn = self.conn()?;
        let count: i64 = if let Some(at) = cursor {
            conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE contact_id = ?1 AND direction = 'in' AND created_at > ?2",
                params![contact_id, at.to_rfc3339()],
                |r| r.get(0),
            )?
        } else {
            conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE contact_id = ?1 AND direction = 'in'",
                params![contact_id],
                |r| r.get(0),
            )?
        };
        Ok(count as usize)
    }

    pub fn unread_counts_all(&self) -> Result<std::collections::HashMap<String, usize>, StorageError> {
        let mut map = std::collections::HashMap::new();
        for c in self.list_contacts()? {
            map.insert(c.user_id.clone(), self.count_unread(&c.user_id)?);
        }
        Ok(map)
    }

    pub fn latest_chat_previews(&self) -> Result<Vec<ChatPreview>, StorageError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT m.contact_id, m.body, m.kind, m.created_at, m.direction, m.attachment_name
             FROM messages m
             INNER JOIN (
               SELECT contact_id, MAX(created_at) AS max_at
               FROM messages GROUP BY contact_id
             ) latest ON m.contact_id = latest.contact_id AND m.created_at = latest.max_at",
        )?;
        let rows = stmt.query_map([], |row| {
            let created_at: String = row.get(3)?;
            Ok(ChatPreview {
                contact_id: row.get(0)?,
                preview: row.get(1)?,
                kind: row.get::<_, Option<String>>(2)?.unwrap_or_else(|| "text".into()),
                created_at: created_at.parse().unwrap_or_else(|_| Utc::now()),
                direction: row.get(4)?,
                attachment_name: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatPreview {
    pub contact_id: String,
    pub preview: String,
    pub kind: String,
    pub created_at: DateTime<Utc>,
    pub direction: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use corgigram_crypto::Identity;

    fn test_bundle(user_id: &str) -> corgigram_crypto::PreKeyBundle {
        Identity::generate(user_id, user_id).prekey_bundle()
    }

    fn sample_message(id: &str, contact: &str, direction: &str, body: &str, at: DateTime<Utc>) -> MessageRecord {
        MessageRecord {
            id: id.into(),
            contact_id: contact.into(),
            direction: direction.into(),
            body: body.into(),
            status: "delivered".into(),
            created_at: at,
            kind: "text".into(),
            attachment_name: None,
            attachment_mime: None,
        }
    }

    #[test]
    fn unread_counts_inbound_after_read_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("t.db");
        let storage = Storage::open(&db).unwrap();
        let t0 = Utc::now();
        storage
            .upsert_contact(&ContactRecord {
                user_id: "alice".into(),
                display_name: "Alice".into(),
                bundle: test_bundle("alice"),
                created_at: t0,
                avatar_data_url: None,
            })
            .unwrap();
        storage
            .insert_message(&sample_message("m1", "alice", "in", "hi", t0))
            .unwrap();
        assert_eq!(storage.count_unread("alice").unwrap(), 1);
        storage.set_read_cursor("alice", t0).unwrap();
        assert_eq!(storage.count_unread("alice").unwrap(), 0);
        storage
            .insert_message(&sample_message("m2", "alice", "in", "again", t0 + Duration::seconds(1)))
            .unwrap();
        assert_eq!(storage.count_unread("alice").unwrap(), 1);
    }

    #[test]
    fn outbound_messages_do_not_affect_unread() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(&dir.path().join("t.db")).unwrap();
        let t0 = Utc::now();
        storage
            .upsert_contact(&ContactRecord {
                user_id: "bob".into(),
                display_name: "Bob".into(),
                bundle: test_bundle("bob"),
                created_at: t0,
                avatar_data_url: None,
            })
            .unwrap();
        storage
            .insert_message(&sample_message("o1", "bob", "out", "sent", t0))
            .unwrap();
        assert_eq!(storage.count_unread("bob").unwrap(), 0);
    }

    #[test]
    fn latest_chat_preview_returns_most_recent() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(&dir.path().join("t.db")).unwrap();
        let t0 = Utc::now();
        storage
            .upsert_contact(&ContactRecord {
                user_id: "c1".into(),
                display_name: "C".into(),
                bundle: test_bundle("c1"),
                created_at: t0,
                avatar_data_url: None,
            })
            .unwrap();
        storage
            .insert_message(&sample_message("a", "c1", "in", "first", t0))
            .unwrap();
        storage
            .insert_message(&sample_message("b", "c1", "out", "second", t0 + Duration::seconds(5)))
            .unwrap();
        let previews = storage.latest_chat_previews().unwrap();
        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].preview, "second");
        assert_eq!(previews[0].direction, "out");
    }
}

fn map_message_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageRecord> {
    let created_at: String = row.get(5)?;
    Ok(MessageRecord {
        id: row.get(0)?,
        contact_id: row.get(1)?,
        direction: row.get(2)?,
        body: row.get(3)?,
        status: row.get(4)?,
        created_at: created_at.parse().unwrap_or_else(|_| Utc::now()),
        kind: row.get::<_, Option<String>>(6)?.unwrap_or_else(|| "text".into()),
        attachment_name: row.get(7)?,
        attachment_mime: row.get(8)?,
    })
}
