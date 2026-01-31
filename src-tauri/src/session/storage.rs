// Session folder management

use super::SessionMetadata;
use std::fs;

/// Save session metadata to disk using atomic write
/// 
/// Writes to a temporary file first, then renames to the final path.
/// This prevents data corruption if the write is interrupted.
pub fn save_metadata(metadata: &SessionMetadata) -> anyhow::Result<()> {
    let metadata_path = metadata.path.join("metadata.json");
    let temp_path = metadata.path.join(".metadata.json.tmp");
    let contents = serde_json::to_string_pretty(metadata)?;
    
    // Write to temporary file first
    fs::write(&temp_path, &contents)?;
    
    // Atomically rename to final path (atomic on most filesystems)
    fs::rename(&temp_path, &metadata_path)?;
    
    Ok(())
}
