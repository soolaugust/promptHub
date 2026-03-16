use crate::error::{RegistryError, Result};
use rusqlite::{Connection, params};
use std::sync::{Arc, Mutex};

/// Thread-safe SQLite connection pool (single connection with Mutex for simplicity).
#[derive(Clone)]
pub struct Db {
    /// pub(crate) for test helpers that need to inject raw SQL (e.g. simulate revoked tokens).
    pub(crate) conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &str) -> Result<Self> {
        let conn = if path == ":memory:" {
            Connection::open_in_memory()?
        } else {
            Connection::open(path)?
        };
        let db = Db { conn: Arc::new(Mutex::new(conn)) };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(r#"
            CREATE TABLE IF NOT EXISTS users (
                id            INTEGER PRIMARY KEY,
                username      TEXT UNIQUE NOT NULL,
                password_hash TEXT NOT NULL,
                created_at    TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS tokens (
                id         INTEGER PRIMARY KEY,
                token      TEXT UNIQUE NOT NULL,
                user_id    INTEGER REFERENCES users(id),
                name       TEXT,
                expires_at TEXT,
                revoked    INTEGER DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS layer_meta (
                id          INTEGER PRIMARY KEY,
                namespace   TEXT NOT NULL,
                name        TEXT NOT NULL,
                version     TEXT NOT NULL,
                description TEXT,
                tags        TEXT,
                pushed_by   INTEGER REFERENCES users(id),
                pushed_at   TEXT NOT NULL,
                UNIQUE(namespace, name, version)
            );
        "#)?;
        Ok(())
    }

    // ── users ──────────────────────────────────────────────────────────────

    pub fn create_user(&self, username: &str, password_hash: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO users (username, password_hash, created_at) VALUES (?1, ?2, ?3)",
            params![username, password_hash, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_user_by_username(&self, username: &str) -> Result<Option<(i64, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, password_hash FROM users WHERE username = ?1"
        )?;
        let mut rows = stmt.query(params![username])?;
        if let Some(row) = rows.next()? {
            Ok(Some((row.get(0)?, row.get(1)?)))
        } else {
            Ok(None)
        }
    }

    // ── tokens ─────────────────────────────────────────────────────────────

    pub fn insert_token(&self, token: &str, user_id: Option<i64>, name: Option<&str>, expires_at: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tokens (token, user_id, name, expires_at) VALUES (?1, ?2, ?3, ?4)",
            params![token, user_id, name, expires_at],
        )?;
        Ok(())
    }

    /// Returns (user_id, name) if token is valid (not revoked, not expired).
    pub fn validate_token(&self, token: &str) -> Result<Option<(Option<i64>, Option<String>)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT user_id, name, expires_at, revoked FROM tokens WHERE token = ?1"
        )?;
        let mut rows = stmt.query(params![token])?;
        if let Some(row) = rows.next()? {
            let revoked: i32 = row.get(3)?;
            if revoked != 0 {
                return Ok(None);
            }
            let expires_at: Option<String> = row.get(2)?;
            if let Some(ref exp) = expires_at {
                let exp_time = chrono::DateTime::parse_from_rfc3339(exp)
                    .map_err(|e| RegistryError::Internal(e.to_string()))?;
                if exp_time < chrono::Utc::now() {
                    return Ok(None); // expired
                }
            }
            let user_id: Option<i64> = row.get(0)?;
            let name: Option<String> = row.get(1)?;
            Ok(Some((user_id, name)))
        } else {
            Ok(None)
        }
    }

    // ── layer_meta ─────────────────────────────────────────────────────────

    pub fn layer_exists(&self, namespace: &str, name: &str, version: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM layer_meta WHERE namespace=?1 AND name=?2 AND version=?3",
            params![namespace, name, version],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn insert_layer(&self, namespace: &str, name: &str, version: &str,
                         description: Option<&str>, tags: &[String], pushed_by: Option<i64>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let tags_json = serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "INSERT INTO layer_meta (namespace, name, version, description, tags, pushed_by, pushed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![namespace, name, version, description, tags_json, pushed_by, now],
        )?;
        Ok(())
    }

    pub fn list_layers(&self) -> Result<Vec<LayerSummary>> {
        let conn = self.conn.lock().unwrap();
        // Order by pushed_at ASC so the last entry per (namespace, name) is the most recently pushed
        let mut stmt = conn.prepare(
            "SELECT namespace, name, version FROM layer_meta ORDER BY namespace, name, pushed_at ASC"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;

        let mut map: std::collections::BTreeMap<(String, String), Vec<String>> = Default::default();
        for row in rows {
            let (ns, nm, ver) = row?;
            map.entry((ns, nm)).or_default().push(ver);
        }

        Ok(map.into_iter().map(|((namespace, name), versions)| {
            // `versions` is in ascending pushed_at order; last() is the most recently pushed
            let latest = versions.last().cloned().unwrap_or_default();
            LayerSummary { namespace, name, latest, versions }
        }).collect())
    }

    pub fn search_layers(&self, query: &str) -> Result<Vec<LayerSummary>> {
        let pattern = format!("%{}%", query.to_lowercase());
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT namespace, name, version FROM layer_meta
             WHERE LOWER(name) LIKE ?1 OR LOWER(namespace) LIKE ?1 OR LOWER(description) LIKE ?1
             ORDER BY namespace, name, pushed_at ASC"
        )?;
        let rows = stmt.query_map(params![pattern], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;

        let mut map: std::collections::BTreeMap<(String, String), Vec<String>> = Default::default();
        for row in rows {
            let (ns, nm, ver) = row?;
            map.entry((ns, nm)).or_default().push(ver);
        }

        Ok(map.into_iter().map(|((namespace, name), versions)| {
            let latest = versions.last().cloned().unwrap_or_default();
            LayerSummary { namespace, name, latest, versions }
        }).collect())
    }

    pub fn get_versions(&self, namespace: &str, name: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT version FROM layer_meta WHERE namespace=?1 AND name=?2 ORDER BY version"
        )?;
        let rows = stmt.query_map(params![namespace, name], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<String>>>()?)
    }
}

#[derive(Debug, serde::Serialize)]
pub struct LayerSummary {
    pub namespace: String,
    pub name: String,
    pub latest: String,
    pub versions: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Db { Db::open(":memory:").unwrap() }

    #[test]
    fn test_create_and_get_user() {
        let db = test_db();
        db.create_user("alice", "hashed_pw").unwrap();
        let user = db.get_user_by_username("alice").unwrap();
        assert!(user.is_some());
        let (_, hash) = user.unwrap();
        assert_eq!(hash, "hashed_pw");
    }

    #[test]
    fn test_get_unknown_user_returns_none() {
        let db = test_db();
        assert!(db.get_user_by_username("nobody").unwrap().is_none());
    }

    #[test]
    fn test_token_valid() {
        let db = test_db();
        db.insert_token("phrt_test", None, Some("ci"), None).unwrap();
        let result = db.validate_token("phrt_test").unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn test_token_unknown_returns_none() {
        let db = test_db();
        assert!(db.validate_token("phrt_unknown").unwrap().is_none());
    }

    #[test]
    fn test_token_revoked_is_invalid() {
        let db = test_db();
        db.insert_token("phrt_revoke_me", None, Some("ci"), None).unwrap();
        // Manually revoke by direct SQL update
        let conn = db.conn.lock().unwrap();
        conn.execute("UPDATE tokens SET revoked = 1 WHERE token = 'phrt_revoke_me'", []).unwrap();
        drop(conn);
        assert!(db.validate_token("phrt_revoke_me").unwrap().is_none());
    }

    #[test]
    fn test_token_expired_is_invalid() {
        let db = test_db();
        // Insert a token that expired in the past
        let past = "2000-01-01T00:00:00Z";
        db.insert_token("phrt_old", None, Some("ci"), Some(past)).unwrap();
        assert!(db.validate_token("phrt_old").unwrap().is_none());
    }

    #[test]
    fn test_layer_exists_after_insert() {
        let db = test_db();
        assert!(!db.layer_exists("base", "expert", "v1.0").unwrap());
        db.insert_layer("base", "expert", "v1.0", Some("desc"), &[], None).unwrap();
        assert!(db.layer_exists("base", "expert", "v1.0").unwrap());
    }

    #[test]
    fn test_list_layers() {
        let db = test_db();
        db.insert_layer("base", "expert", "v1.0", Some("desc"), &[], None).unwrap();
        db.insert_layer("base", "expert", "v2.0", Some("desc"), &[], None).unwrap();
        db.insert_layer("style", "concise", "v1.0", Some("desc"), &[], None).unwrap();
        let layers = db.list_layers().unwrap();
        assert_eq!(layers.len(), 2);
        // BTreeMap sorts alphabetically: "base/expert" before "style/concise"
        assert_eq!(layers[0].name, "expert");
        assert_eq!(layers[0].versions.len(), 2);
        assert!(layers[0].versions.contains(&"v1.0".to_string()));
        assert!(layers[0].versions.contains(&"v2.0".to_string()));
    }

    #[test]
    fn test_search_layers() {
        let db = test_db();
        db.insert_layer("base", "code-reviewer", "v1.0", Some("reviews code"), &[], None).unwrap();
        db.insert_layer("style", "concise", "v1.0", Some("brief output"), &[], None).unwrap();
        let results = db.search_layers("code").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "code-reviewer");
    }

    #[test]
    fn test_get_versions() {
        let db = test_db();
        db.insert_layer("base", "expert", "v1.0", None, &[], None).unwrap();
        db.insert_layer("base", "expert", "v2.0", None, &[], None).unwrap();
        let versions = db.get_versions("base", "expert").unwrap();
        assert_eq!(versions, vec!["v1.0", "v2.0"]);
    }
}
