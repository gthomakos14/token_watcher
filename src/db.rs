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
                            if let Some(id) = crate::parser::get_first_field(&id_bytes, 2).and_then(|v| v.as_varint()) {
                                return Some(id as u32);
                            }
                            if let Some(id) = crate::parser::get_first_field(&id_bytes, 1).and_then(|v| v.as_varint()) {
                                return Some(id as u32);
                            }
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
            let key: String = row.get("key")?;
            let value: String = row.get("value")?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::tests::{field_bytes, field_varint, varint};

    const SENTINEL: &str = "last_selected_agent_model_sentinel_key";

    /// Builds modelPreferences as stored: base64(1 { 1: key, 2 { 1: base64(id_bytes) } }).
    fn prefs(key: &str, id_bytes: &[u8]) -> String {
        let value = field_bytes(1, BASE64.encode(id_bytes).as_bytes());
        let entry = [field_bytes(1, key.as_bytes()), field_bytes(2, &value)].concat();
        BASE64.encode(field_bytes(1, &entry))
    }

    #[test]
    fn reads_the_id_from_field_2() {
        assert_eq!(parse_selected_model_id(&prefs(SENTINEL, &field_varint(2, 42))), Some(42));
    }

    #[test]
    fn falls_back_to_field_1() {
        assert_eq!(parse_selected_model_id(&prefs(SENTINEL, &field_varint(1, 7))), Some(7));
    }

    #[test]
    fn falls_back_to_a_bare_varint() {
        // 0x80 0x01 = 128 as a raw varint; as a protobuf key it would be field 16, so
        // neither field 1 nor 2 matches.
        assert_eq!(parse_selected_model_id(&prefs(SENTINEL, &varint(128))), Some(128));
    }

    #[test]
    fn finds_the_sentinel_among_other_entries() {
        let other = field_bytes(1, &field_bytes(1, b"some_other_key"));
        let wanted = BASE64.decode(prefs(SENTINEL, &field_varint(2, 5))).unwrap();
        let raw = BASE64.encode([other, wanted].concat());
        assert_eq!(parse_selected_model_id(&raw), Some(5));
    }

    #[test]
    fn other_keys_and_bad_input_give_none() {
        assert_eq!(parse_selected_model_id(&prefs("another_key", &field_varint(2, 42))), None);
        assert_eq!(parse_selected_model_id("not base64!"), None);
        assert_eq!(parse_selected_model_id(""), None);
        assert_eq!(parse_selected_model_id(&prefs(SENTINEL, &[])), None);
    }

    #[test]
    fn missing_database_is_an_error() {
        let err = read_token_report(Path::new("/nonexistent/state.vscdb")).unwrap_err();
        assert!(err.contains("does not exist"));
    }
}
