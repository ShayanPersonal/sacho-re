// SQLite session index for fast queries

use super::{SessionMetadata, SessionSummary};
use rusqlite::{Connection, params};
use std::path::PathBuf;
use parking_lot::Mutex;
use tauri::{AppHandle, Manager};
use chrono::{DateTime, Utc};

/// Session database for fast queries
/// 
/// Wraps Connection in a parking_lot::Mutex since rusqlite::Connection is not Sync.
/// Using parking_lot instead of std::sync::Mutex to avoid mutex poisoning on panic,
/// which would make all subsequent database operations fail.
pub struct SessionDatabase {
    conn: Mutex<Connection>,
}

impl SessionDatabase {
    /// Open or create the session database
    pub fn open(app_handle: &AppHandle) -> anyhow::Result<Self> {
        let db_path = app_handle
            .path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("sessions.db");
        
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let conn = Connection::open(&db_path)?;
        
        let db = Self { conn: Mutex::new(conn) };
        db.init_schema()?;
        
        Ok(db)
    }
    
    /// Open an in-memory database (fallback when file database fails)
    pub fn open_in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        
        let db = Self { conn: Mutex::new(conn) };
        db.init_schema()?;
        
        log::warn!("Using in-memory database - sessions will not persist across restarts");
        
        Ok(db)
    }
    
    /// Initialize database schema
    fn init_schema(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute_batch(r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                timestamp TEXT NOT NULL,
                duration_secs REAL NOT NULL,
                path TEXT NOT NULL,
                has_audio INTEGER NOT NULL DEFAULT 0,
                has_midi INTEGER NOT NULL DEFAULT 0,
                has_video INTEGER NOT NULL DEFAULT 0,
                notes TEXT NOT NULL DEFAULT '',
                notes_modified_at TEXT NOT NULL DEFAULT '',
                title TEXT
            );

            CREATE TABLE IF NOT EXISTS midi_imports (
                id TEXT PRIMARY KEY,
                folder_path TEXT NOT NULL,
                file_name TEXT NOT NULL,
                file_path TEXT NOT NULL,
                chunked_features BLOB,
                has_features INTEGER NOT NULL DEFAULT 0,
                imported_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS session_features (
                session_id TEXT PRIMARY KEY,
                chunked_features BLOB,
                has_features INTEGER NOT NULL DEFAULT 0,
                midi_file_count INTEGER NOT NULL DEFAULT 0,
                computed_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_sessions_timestamp ON sessions(timestamp DESC);
            -- Full-text search for notes
            CREATE VIRTUAL TABLE IF NOT EXISTS sessions_fts USING fts5(
                id,
                notes,
                content='sessions',
                content_rowid='rowid'
            );
        "#)?;

        // Migration: add notes_modified_at column for existing databases
        let has_column: bool = conn
            .prepare("SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'notes_modified_at'")?
            .query_row([], |row| row.get::<_, i64>(0))
            .map(|count| count > 0)?;

        if !has_column {
            conn.execute_batch("ALTER TABLE sessions ADD COLUMN notes_modified_at TEXT NOT NULL DEFAULT ''")?;
        }

        // Migration: add title column for existing databases
        let has_title: bool = conn
            .prepare("SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'title'")?
            .query_row([], |row| row.get::<_, i64>(0))
            .map(|count| count > 0)?;

        if !has_title {
            conn.execute_batch("ALTER TABLE sessions ADD COLUMN title TEXT")?;
        }

        Ok(())
    }
    
    /// Insert or update a session
    pub fn upsert_session(&self, metadata: &SessionMetadata) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            r#"
            INSERT INTO sessions (
                id, timestamp, duration_secs, path, has_audio, has_midi, has_video,
                notes, notes_modified_at, title
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, '', ?9)
            ON CONFLICT(id) DO UPDATE SET
                timestamp = excluded.timestamp,
                duration_secs = excluded.duration_secs,
                path = excluded.path,
                has_audio = excluded.has_audio,
                has_midi = excluded.has_midi,
                has_video = excluded.has_video,
                notes = excluded.notes,
                title = excluded.title
            "#,
            params![
                metadata.id,
                metadata.timestamp.to_rfc3339(),
                metadata.duration_secs,
                metadata.path.to_string_lossy().to_string(),
                !metadata.audio_files.is_empty(),
                !metadata.midi_files.is_empty(),
                !metadata.video_files.is_empty(),
                metadata.notes,
                metadata.title,
            ],
        )?;

        Ok(())
    }
    
    /// Get all existing session rows for lightweight comparison during rescan
    pub fn get_all_existing_sessions(&self) -> anyhow::Result<Vec<ExistingSessionRow>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, has_audio, has_midi, has_video, notes_modified_at FROM sessions"
        )?;

        let mut rows_out = Vec::new();
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            rows_out.push(ExistingSessionRow {
                id: row.get(0)?,
                has_audio: row.get(1)?,
                has_midi: row.get(2)?,
                has_video: row.get(3)?,
                notes_modified_at: row.get(4)?,
            });
        }
        Ok(rows_out)
    }

    /// Sync new, updated, and deleted sessions in a single transaction
    pub fn batch_sync(
        &self,
        new: &[SessionIndexData],
        updated: &[UpdatedSessionData],
        deleted_ids: &[&String],
    ) -> anyhow::Result<usize> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let mut count = 0;

        for s in new {
            tx.execute(
                r#"
                INSERT INTO sessions (
                    id, timestamp, duration_secs, path, has_audio, has_midi, has_video,
                    notes, notes_modified_at, title
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT(id) DO UPDATE SET
                    timestamp = excluded.timestamp,
                    duration_secs = excluded.duration_secs,
                    path = excluded.path,
                    has_audio = excluded.has_audio,
                    has_midi = excluded.has_midi,
                    has_video = excluded.has_video,
                    notes = excluded.notes,
                    notes_modified_at = excluded.notes_modified_at,
                    title = excluded.title
                "#,
                params![
                    s.id,
                    s.timestamp.to_rfc3339(),
                    s.duration_secs,
                    s.path,
                    s.has_audio,
                    s.has_midi,
                    s.has_video,
                    s.notes,
                    s.notes_modified_at,
                    s.title,
                ],
            )?;
            count += 1;
        }

        for u in updated {
            tx.execute(
                r#"
                UPDATE sessions SET
                    has_audio = ?1,
                    has_midi = ?2,
                    has_video = ?3,
                    notes = ?4,
                    notes_modified_at = ?5,
                    title = ?6
                WHERE id = ?7
                "#,
                params![
                    u.has_audio,
                    u.has_midi,
                    u.has_video,
                    u.notes,
                    u.notes_modified_at,
                    u.title,
                    u.id,
                ],
            )?;
            count += 1;
        }

        for id in deleted_ids {
            tx.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
            tx.execute("DELETE FROM session_features WHERE session_id = ?1", params![id])?;
            count += 1;
        }

        tx.commit()?;
        Ok(count)
    }

    /// Update notes and modified timestamp for a session
    pub fn update_notes_with_timestamp(
        &self,
        session_id: &str,
        notes: &str,
        notes_modified_at: &str,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE sessions SET notes = ?1, notes_modified_at = ?2 WHERE id = ?3",
            params![notes, notes_modified_at, session_id],
        )?;
        Ok(())
    }

    /// Rename a session (update ID, path, and title)
    pub fn rename_session(&self, old_id: &str, new_id: &str, new_path: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE sessions SET id = ?1, path = ?2, title = ?3 WHERE id = ?4",
            params![new_id, new_path, super::extract_title_from_folder_name(new_id), old_id],
        )?;
        conn.execute(
            "UPDATE session_features SET session_id = ?1 WHERE session_id = ?2",
            params![new_id, old_id],
        )?;
        Ok(())
    }

    /// Delete a session from the index
    pub fn delete_session(&self, session_id: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM sessions WHERE id = ?1",
            params![session_id],
        )?;
        conn.execute(
            "DELETE FROM session_features WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
    }
    
    /// Query sessions with filters
    pub fn query_sessions(&self, filter: &SessionFilter) -> anyhow::Result<Vec<SessionSummary>> {
        let mut sql = String::from(
            r#"
            SELECT s.id, s.timestamp, s.duration_secs, s.has_audio, s.has_midi, s.has_video,
                   s.notes, s.title
            FROM sessions s
            WHERE 1=1
            "#
        );

        // Build search query if provided
        let search_pattern = filter.search_query.as_ref().map(|q| format!("%{}%", q));

        if search_pattern.is_some() {
            sql.push_str(" AND (s.notes LIKE ?1 OR s.title LIKE ?1)");
        }
        
        if filter.has_audio == Some(true) {
            sql.push_str(" AND s.has_audio = 1");
        }
        
        if filter.has_midi == Some(true) {
            sql.push_str(" AND s.has_midi = 1");
        }
        
        if filter.has_video == Some(true) {
            sql.push_str(" AND s.has_video = 1");
        }
        
        if filter.has_notes == Some(true) {
            sql.push_str(" AND s.notes IS NOT NULL AND s.notes != ''");
        }

        if filter.has_title == Some(true) {
            sql.push_str(" AND s.title IS NOT NULL AND s.title != ''");
        }

        sql.push_str(" ORDER BY s.timestamp DESC");
        
        if let Some(limit) = filter.limit {
            sql.push_str(&format!(" LIMIT {}", limit));
        }
        
        if let Some(offset) = filter.offset {
            sql.push_str(&format!(" OFFSET {}", offset));
        }
        
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&sql)?;
        
        let mut sessions = Vec::new();
        
        if let Some(ref pattern) = search_pattern {
            let mut rows = stmt.query([pattern])?;
            while let Some(row) = rows.next()? {
                sessions.push(Self::map_session_row(row)?);
            }
        } else {
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                sessions.push(Self::map_session_row(row)?);
            }
        };
        
        Ok(sessions)
    }
    
    fn map_session_row(row: &rusqlite::Row) -> rusqlite::Result<SessionSummary> {
        let timestamp_str: String = row.get(1)?;
        let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|e| {
                log::warn!("Failed to parse timestamp '{}': {}, using current time", timestamp_str, e);
                Utc::now()
            });

        Ok(SessionSummary {
            id: row.get(0)?,
            timestamp,
            duration_secs: row.get(2)?,
            has_audio: row.get(3)?,
            has_midi: row.get(4)?,
            has_video: row.get(5)?,
            notes: row.get(6)?,
            title: row.get(7)?,
        })
    }
    

    /// Insert MIDI imports in a batch
    pub fn insert_midi_imports(&self, imports: &[MidiImport]) -> anyhow::Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;

        for import in imports {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO midi_imports (
                    id, folder_path, file_name, file_path, chunked_features, has_features, imported_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                "#,
                params![
                    import.id,
                    import.folder_path,
                    import.file_name,
                    import.file_path,
                    import.chunked_features,
                    import.has_features,
                    import.imported_at,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Get all MIDI imports with features
    pub fn get_all_midi_imports(&self) -> anyhow::Result<Vec<MidiImport>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, folder_path, file_name, file_path, chunked_features, has_features, imported_at FROM midi_imports"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(MidiImport {
                id: row.get(0)?,
                folder_path: row.get(1)?,
                file_name: row.get(2)?,
                file_path: row.get(3)?,
                chunked_features: row.get(4)?,
                has_features: row.get(5)?,
                imported_at: row.get(6)?,
            })
        })?;

        let mut imports = Vec::new();
        for row in rows {
            imports.push(row?);
        }
        Ok(imports)
    }

    /// Get MIDI import metadata without loading feature blobs
    pub fn get_midi_import_list(&self) -> anyhow::Result<Vec<MidiImport>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, folder_path, file_name, file_path, has_features, imported_at FROM midi_imports"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(MidiImport {
                id: row.get(0)?,
                folder_path: row.get(1)?,
                file_name: row.get(2)?,
                file_path: row.get(3)?,
                chunked_features: None,
                has_features: row.get(4)?,
                imported_at: row.get(5)?,
            })
        })?;

        let mut imports = Vec::new();
        for row in rows {
            imports.push(row?);
        }
        Ok(imports)
    }

    /// Clear all MIDI imports
    pub fn clear_midi_imports(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM midi_imports", [])?;
        conn.execute_batch("VACUUM")?;
        Ok(())
    }

    /// Clear all sessions (cache reset)
    pub fn clear_sessions(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM sessions", [])?;
        // Rebuild FTS index after content table is cleared
        conn.execute("INSERT INTO sessions_fts(sessions_fts) VALUES('rebuild')", [])?;
        conn.execute("DELETE FROM midi_imports", [])?;
        conn.execute("DELETE FROM session_features", [])?;
        conn.execute_batch("VACUUM")?;
        Ok(())
    }

    /// Get all session feature rows
    pub fn get_all_session_features(&self) -> anyhow::Result<Vec<SessionFeatureRow>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT session_id, chunked_features, has_features, midi_file_count, computed_at FROM session_features"
        )?;

        let mut rows = Vec::new();
        let mut result = stmt.query([])?;
        while let Some(row) = result.next()? {
            rows.push(SessionFeatureRow {
                session_id: row.get(0)?,
                chunked_features: row.get(1)?,
                has_features: row.get(2)?,
                midi_file_count: row.get(3)?,
                computed_at: row.get(4)?,
            });
        }
        Ok(rows)
    }

    /// Insert or replace a single session feature
    pub fn upsert_session_feature(&self, feature: &SessionFeatureRow) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            r#"
            INSERT OR REPLACE INTO session_features (
                session_id, chunked_features, has_features, midi_file_count, computed_at
            ) VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                feature.session_id,
                feature.chunked_features,
                feature.has_features,
                feature.midi_file_count,
                feature.computed_at,
            ],
        )?;
        Ok(())
    }

    /// Batch insert/replace session features in a single transaction
    pub fn upsert_session_features_batch(&self, features: &[SessionFeatureRow]) -> anyhow::Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;

        for f in features {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO session_features (
                    session_id, chunked_features, has_features, midi_file_count, computed_at
                ) VALUES (?1, ?2, ?3, ?4, ?5)
                "#,
                params![
                    f.session_id,
                    f.chunked_features,
                    f.has_features,
                    f.midi_file_count,
                    f.computed_at,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Delete session features for given session IDs
    pub fn delete_session_features_by_ids(&self, ids: &[&str]) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        for id in ids {
            conn.execute(
                "DELETE FROM session_features WHERE session_id = ?1",
                params![id],
            )?;
        }
        Ok(())
    }
}

/// Filter for session queries
#[derive(Debug, Clone, Default)]
pub struct SessionFilter {
    pub search_query: Option<String>,
    pub has_audio: Option<bool>,
    pub has_midi: Option<bool>,
    pub has_video: Option<bool>,
    pub has_notes: Option<bool>,
    pub has_title: Option<bool>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Lightweight session data for initial index (new sessions only)
pub struct SessionIndexData {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub path: String,
    pub duration_secs: f64,
    pub has_audio: bool,
    pub has_midi: bool,
    pub has_video: bool,
    pub notes: String,
    pub notes_modified_at: String,
    pub title: Option<String>,
}

/// Existing session row for lightweight comparison during rescan
pub struct ExistingSessionRow {
    pub id: String,
    pub has_audio: bool,
    pub has_midi: bool,
    pub has_video: bool,
    pub notes_modified_at: String,
}

/// Tag/notes-only update data (no duration recompute)
pub struct UpdatedSessionData {
    pub id: String,
    pub has_audio: bool,
    pub has_midi: bool,
    pub has_video: bool,
    pub notes: String,
    pub notes_modified_at: String,
    pub title: Option<String>,
}

/// Precomputed features for a recording session (similarity analysis)
#[derive(Debug, Clone)]
pub struct SessionFeatureRow {
    pub session_id: String,
    pub chunked_features: Option<Vec<u8>>,
    pub has_features: bool,
    pub midi_file_count: i32,
    pub computed_at: String,
}

/// Imported MIDI file for similarity analysis
#[derive(Debug, Clone)]
pub struct MidiImport {
    pub id: String,
    pub folder_path: String,
    pub file_name: String,
    pub file_path: String,
    pub chunked_features: Option<Vec<u8>>,
    pub has_features: bool,
    pub imported_at: String,
}

