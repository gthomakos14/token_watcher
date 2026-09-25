use crate::models::TokenReport;
use crate::parser::parse_user_status;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::env::VarError;

pub fn default_db_path() -> Result<PathBuf, VarError> {
    let home = std::env::var("HOME")?;
    Ok(
        PathBuf::from(home)
            .join(".config")
            .join("Antigravity")
            .join("User")
            .join("globalStorage")
            .join("state.vscdb"),
    )
}

pub fn parse_selected_model_id(pref_raw: &str) -> Option<u32> {
    // pref_raw is base64
    let bytes = BASE64.decode(pref_raw.trim()).ok()?;
    // Field 1 is tag, Field 2 is value with subfield
    // Look for sentinel string: last_selected_agent_model_sentinel_key
    // Value in field 2 has length-delimited base64 or varint
    let fields = crate::parser::get_fields(&bytes, 1);
    for f in fields {
        if let Some(chunk) = f.as_bytes() {
            // inside field 1, check subfields
            let key = crate::parser::get_first_field(chunk, 1).and_then(|v| v.as_str().map(|s| s.to_string()));
            if key.as_deref() == Some("last_selected_agent_model_sentinel_key") {
                if let Some(val_bytes) = crate::parser::get_first_field(chunk, 2).and_then(|v| v.as_bytes()) {
                    // val_bytes has field 1 (string)
                    if let Some(inner_b64) = crate::parser::get_first_field(val_bytes, 1).and_then(|v| v.as_str()) {
                        if let Ok(id_bytes) = BASE64.decode(inner_b64) {
                            let mut offset = 0;
                            if let Some(id) = crate::parser::read_varint(&id_bytes, &mut offset) {
                                return Some(id as u32);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

pub fn read_token_report(db_path: &Path) -> Result<TokenReport, String> {
    if !db_path.exists() {
        return Err(format!("Database file does not exist: {}", db_path.display()));
    }

    // Open in read-only mode with URI
    let uri = format!("file:{}?mode=ro", db_path.to_string_lossy());
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
    let conn = Connection::open_with_flags(&uri, flags)
        .map_err(|e| format!("Failed to open SQLite db: {}", e))?;

    let mut stmt = conn
        .prepare("SELECT key, value FROM ItemTable WHERE key IN ('antigravityUnifiedStateSync.userStatus', 'antigravityUnifiedStateSync.modelPreferences')")
        .map_err(|e| format!("Failed to prepare SQL statement: {}", e))?;

    let rows = stmt
        .query_map([], |row| {
            let key: String = row.get(0)?;
            let value: String = row.get(1)?;
            Ok((key, value))
        })
        .map_err(|e| format!("Failed to query ItemTable: {}", e))?;

    let mut user_status_raw: Option<String> = None;
    let mut model_pref_raw: Option<String> = None;

    for row in rows.filter_map(Result::ok) {
        let key = row.0;
        let val = row.1;
        if key == "antigravityUnifiedStateSync.userStatus" {
            user_status_raw = Some(val);
        } else if key == "antigravityUnifiedStateSync.modelPreferences" {
            model_pref_raw = Some(val);
        }
    }

    let raw_status = user_status_raw.ok_or("antigravityUnifiedStateSync.userStatus key not found in state.vscdb")?;
    let selected_id = model_pref_raw.as_deref().and_then(parse_selected_model_id);

    parse_user_status(&raw_status, selected_id)
}
