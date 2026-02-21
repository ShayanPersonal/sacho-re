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
                notes TEXT NOT NULL DEFAULT ''
            );

            CREATE TABLE IF NOT EXISTS midi_imports (
                id TEXT PRIMARY KEY,
                folder_path TEXT NOT NULL,
                file_name TEXT NOT NULL,
                file_path TEXT NOT NULL,
                melodic_features TEXT,
                harmonic_features TEXT,
                imported_at TEXT NOT NULL
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
        
        Ok(())
    }
    
    /// Insert or update a session
    pub fn upsert_session(&self, metadata: &SessionMetadata) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            r#"
            INSERT INTO sessions (
                id, timestamp, duration_secs, path, has_audio, has_midi, has_video,
                notes
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(id) DO UPDATE SET
                timestamp = excluded.timestamp,
                duration_secs = excluded.duration_secs,
                path = excluded.path,
                has_audio = excluded.has_audio,
                has_midi = excluded.has_midi,
                has_video = excluded.has_video,
                notes = excluded.notes
            "#,
            params![
                metadata.id,
                metadata.timestamp.to_rfc3339(),
                metadata.duration_secs,
                metadata.path.to_string_lossy().to_string(),
                !metadata.audio_files.is_empty() || metadata.video_files.iter().any(|v| v.has_audio),
                !metadata.midi_files.is_empty(),
                !metadata.video_files.is_empty(),
                metadata.notes,
            ],
        )?;

        Ok(())
    }
    
    /// Batch upsert multiple sessions in a single transaction (much faster)
    pub fn batch_upsert_sessions(&self, sessions: &[SessionMetadata]) -> anyhow::Result<usize> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;

        let mut count = 0;
        for metadata in sessions {
            tx.execute(
                r#"
                INSERT INTO sessions (
                    id, timestamp, duration_secs, path, has_audio, has_midi, has_video,
                    notes
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(id) DO UPDATE SET
                    timestamp = excluded.timestamp,
                    duration_secs = excluded.duration_secs,
                    path = excluded.path,
                    has_audio = excluded.has_audio,
                    has_midi = excluded.has_midi,
                    has_video = excluded.has_video
                "#,
                params![
                    metadata.id,
                    metadata.timestamp.to_rfc3339(),
                    metadata.duration_secs,
                    metadata.path.to_string_lossy().to_string(),
                    !metadata.audio_files.is_empty() || metadata.video_files.iter().any(|v| v.has_audio),
                    !metadata.midi_files.is_empty(),
                    !metadata.video_files.is_empty(),
                    metadata.notes,
                ],
            )?;

            count += 1;
        }

        tx.commit()?;
        Ok(count)
    }
    
    /// Delete a session from the index
    pub fn delete_session(&self, session_id: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM sessions WHERE id = ?1",
            params![session_id],
        )?;
        Ok(())
    }
    
    /// Query sessions with filters
    pub fn query_sessions(&self, filter: &SessionFilter) -> anyhow::Result<Vec<SessionSummary>> {
        let mut sql = String::from(
            r#"
            SELECT s.id, s.timestamp, s.duration_secs, s.has_audio, s.has_midi, s.has_video,
                   s.notes
            FROM sessions s
            WHERE 1=1
            "#
        );
        
        // Build search query if provided
        let search_pattern = filter.search_query.as_ref().map(|q| format!("%{}%", q));
        
        if search_pattern.is_some() {
            sql.push_str(" AND s.notes LIKE ?1");
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
        })
    }
    
    /// Update notes for a session
    pub fn update_notes(&self, session_id: &str, notes: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE sessions SET notes = ?1 WHERE id = ?2",
            params![notes, session_id],
        )?;
        Ok(())
    }

    /// Insert MIDI imports in a batch
    pub fn insert_midi_imports(&self, imports: &[MidiImport]) -> anyhow::Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;

        for import in imports {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO midi_imports (
                    id, folder_path, file_name, file_path, melodic_features, harmonic_features, imported_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                "#,
                params![
                    import.id,
                    import.folder_path,
                    import.file_name,
                    import.file_path,
                    import.melodic_features,
                    import.harmonic_features,
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
            "SELECT id, folder_path, file_name, file_path, melodic_features, harmonic_features, imported_at FROM midi_imports"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(MidiImport {
                id: row.get(0)?,
                folder_path: row.get(1)?,
                file_name: row.get(2)?,
                file_path: row.get(3)?,
                melodic_features: row.get(4)?,
                harmonic_features: row.get(5)?,
                imported_at: row.get(6)?,
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
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Imported MIDI file for similarity analysis
#[derive(Debug, Clone)]
pub struct MidiImport {
    pub id: String,
    pub folder_path: String,
    pub file_name: String,
    pub file_path: String,
    pub melodic_features: Option<String>,
    pub harmonic_features: Option<String>,
    pub imported_at: String,
}

