// SQLite session index for fast queries

use super::{SessionMetadata, SessionSummary, SimilarityCoords};
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
                audio_count INTEGER NOT NULL DEFAULT 0,
                midi_count INTEGER NOT NULL DEFAULT 0,
                video_count INTEGER NOT NULL DEFAULT 0,
                total_size_bytes INTEGER NOT NULL DEFAULT 0,
                is_favorite INTEGER NOT NULL DEFAULT 0,
                notes TEXT NOT NULL DEFAULT '',
                similarity_x REAL,
                similarity_y REAL,
                cluster_id INTEGER
            );
            
            CREATE TABLE IF NOT EXISTS session_tags (
                session_id TEXT NOT NULL,
                tag TEXT NOT NULL,
                PRIMARY KEY (session_id, tag),
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );
            
            CREATE TABLE IF NOT EXISTS clusters (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL DEFAULT ''
            );
            
            CREATE INDEX IF NOT EXISTS idx_sessions_timestamp ON sessions(timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_sessions_favorite ON sessions(is_favorite);
            CREATE INDEX IF NOT EXISTS idx_sessions_cluster ON sessions(cluster_id);
            CREATE INDEX IF NOT EXISTS idx_session_tags_tag ON session_tags(tag);
            
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
        let total_size: u64 = metadata.audio_files.iter().map(|f| f.size_bytes).sum::<u64>()
            + metadata.midi_files.iter().map(|f| f.size_bytes).sum::<u64>()
            + metadata.video_files.iter().map(|f| f.size_bytes).sum::<u64>();
        
        let conn = self.conn.lock();
        conn.execute(
            r#"
            INSERT INTO sessions (
                id, timestamp, duration_secs, path, has_audio, has_midi, has_video,
                audio_count, midi_count, video_count, total_size_bytes, is_favorite,
                notes, similarity_x, similarity_y, cluster_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            ON CONFLICT(id) DO UPDATE SET
                timestamp = excluded.timestamp,
                duration_secs = excluded.duration_secs,
                path = excluded.path,
                has_audio = excluded.has_audio,
                has_midi = excluded.has_midi,
                has_video = excluded.has_video,
                audio_count = excluded.audio_count,
                midi_count = excluded.midi_count,
                video_count = excluded.video_count,
                total_size_bytes = excluded.total_size_bytes,
                is_favorite = excluded.is_favorite,
                notes = excluded.notes,
                similarity_x = excluded.similarity_x,
                similarity_y = excluded.similarity_y,
                cluster_id = excluded.cluster_id
            "#,
            params![
                metadata.id,
                metadata.timestamp.to_rfc3339(),
                metadata.duration_secs,
                metadata.path.to_string_lossy().to_string(),
                !metadata.audio_files.is_empty(),
                !metadata.midi_files.is_empty(),
                !metadata.video_files.is_empty(),
                metadata.audio_files.len(),
                metadata.midi_files.len(),
                metadata.video_files.len(),
                total_size,
                metadata.is_favorite,
                metadata.notes,
                metadata.similarity_coords.map(|c| c.x),
                metadata.similarity_coords.map(|c| c.y),
                metadata.cluster_id,
            ],
        )?;
        
        // Update tags
        conn.execute(
            "DELETE FROM session_tags WHERE session_id = ?1",
            params![metadata.id],
        )?;
        
        for tag in &metadata.tags {
            conn.execute(
                "INSERT INTO session_tags (session_id, tag) VALUES (?1, ?2)",
                params![metadata.id, tag],
            )?;
        }
        
        Ok(())
    }
    
    /// Batch upsert multiple sessions in a single transaction (much faster)
    pub fn batch_upsert_sessions(&self, sessions: &[SessionMetadata]) -> anyhow::Result<usize> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        
        let mut count = 0;
        for metadata in sessions {
            let total_size: u64 = metadata.audio_files.iter().map(|f| f.size_bytes).sum::<u64>()
                + metadata.midi_files.iter().map(|f| f.size_bytes).sum::<u64>()
                + metadata.video_files.iter().map(|f| f.size_bytes).sum::<u64>();
            
            tx.execute(
                r#"
                INSERT INTO sessions (
                    id, timestamp, duration_secs, path, has_audio, has_midi, has_video,
                    audio_count, midi_count, video_count, total_size_bytes, is_favorite,
                    notes, similarity_x, similarity_y, cluster_id
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                ON CONFLICT(id) DO UPDATE SET
                    timestamp = excluded.timestamp,
                    duration_secs = excluded.duration_secs,
                    path = excluded.path,
                    has_audio = excluded.has_audio,
                    has_midi = excluded.has_midi,
                    has_video = excluded.has_video,
                    audio_count = excluded.audio_count,
                    midi_count = excluded.midi_count,
                    video_count = excluded.video_count,
                    total_size_bytes = excluded.total_size_bytes
                "#,
                params![
                    metadata.id,
                    metadata.timestamp.to_rfc3339(),
                    metadata.duration_secs,
                    metadata.path.to_string_lossy().to_string(),
                    !metadata.audio_files.is_empty(),
                    !metadata.midi_files.is_empty(),
                    !metadata.video_files.is_empty(),
                    metadata.audio_files.len(),
                    metadata.midi_files.len(),
                    metadata.video_files.len(),
                    total_size,
                    metadata.is_favorite,
                    metadata.notes,
                    metadata.similarity_coords.map(|c| c.x),
                    metadata.similarity_coords.map(|c| c.y),
                    metadata.cluster_id,
                ],
            )?;
            
            // Only update tags if they exist (skip delete+insert for empty tags)
            if !metadata.tags.is_empty() {
                tx.execute(
                    "DELETE FROM session_tags WHERE session_id = ?1",
                    params![metadata.id],
                )?;
                
                for tag in &metadata.tags {
                    tx.execute(
                        "INSERT INTO session_tags (session_id, tag) VALUES (?1, ?2)",
                        params![metadata.id, tag],
                    )?;
                }
            }
            
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
            SELECT DISTINCT s.id, s.timestamp, s.duration_secs, s.has_audio, s.has_midi, s.has_video,
                   s.audio_count, s.midi_count, s.video_count, s.total_size_bytes, s.is_favorite,
                   s.similarity_x, s.similarity_y, s.cluster_id, s.notes
            FROM sessions s
            LEFT JOIN session_tags t ON s.id = t.session_id
            WHERE 1=1
            "#
        );
        
        // Build search query if provided
        let search_pattern = filter.search_query.as_ref().map(|q| format!("%{}%", q));
        
        if search_pattern.is_some() {
            sql.push_str(" AND s.notes LIKE ?1");
        }
        
        if filter.favorites_only {
            sql.push_str(" AND s.is_favorite = 1");
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
        
        let similarity_x: Option<f32> = row.get(11)?;
        let similarity_y: Option<f32> = row.get(12)?;
        let similarity_coords = match (similarity_x, similarity_y) {
            (Some(x), Some(y)) => Some(SimilarityCoords { x, y }),
            _ => None,
        };
        
        Ok(SessionSummary {
            id: row.get(0)?,
            timestamp,
            duration_secs: row.get(2)?,
            has_audio: row.get(3)?,
            has_midi: row.get(4)?,
            has_video: row.get(5)?,
            audio_count: row.get(6)?,
            midi_count: row.get(7)?,
            video_count: row.get(8)?,
            total_size_bytes: row.get(9)?,
            is_favorite: row.get(10)?,
            tags: Vec::new(), // Will be populated separately if needed
            notes: row.get(14)?,
            similarity_coords,
            cluster_id: row.get(13)?,
        })
    }
    
    /// Get similarity data for all sessions with MIDI
    pub fn get_similarity_data(&self) -> anyhow::Result<Vec<SimilarityPoint>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            r#"
            SELECT id, similarity_x, similarity_y, cluster_id, timestamp
            FROM sessions
            WHERE has_midi = 1 AND similarity_x IS NOT NULL AND similarity_y IS NOT NULL
            "#
        )?;
        
        let rows = stmt.query_map([], |row| {
            let timestamp_str: String = row.get(4)?;
            let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|e| {
                    log::warn!("Failed to parse similarity timestamp '{}': {}, using current time", timestamp_str, e);
                    Utc::now()
                });
            
            Ok(SimilarityPoint {
                id: row.get(0)?,
                x: row.get(1)?,
                y: row.get(2)?,
                cluster_id: row.get(3)?,
                timestamp,
            })
        })?;
        
        let mut points = Vec::new();
        for row in rows {
            points.push(row?);
        }
        
        Ok(points)
    }
    
    /// Update similarity coordinates for a session
    pub fn update_similarity(&self, session_id: &str, coords: SimilarityCoords, cluster_id: Option<i32>) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE sessions SET similarity_x = ?1, similarity_y = ?2, cluster_id = ?3 WHERE id = ?4",
            params![coords.x, coords.y, cluster_id, session_id],
        )?;
        Ok(())
    }
    
    /// Update favorite status for a session
    pub fn update_favorite(&self, session_id: &str, is_favorite: bool) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE sessions SET is_favorite = ?1 WHERE id = ?2",
            params![is_favorite, session_id],
        )?;
        Ok(())
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
}

/// Filter for session queries
#[derive(Debug, Clone, Default)]
pub struct SessionFilter {
    pub search_query: Option<String>,
    pub favorites_only: bool,
    pub has_audio: Option<bool>,
    pub has_midi: Option<bool>,
    pub has_video: Option<bool>,
    pub has_notes: Option<bool>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Point data for similarity visualization
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimilarityPoint {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub cluster_id: Option<i32>,
    pub timestamp: DateTime<Utc>,
}
