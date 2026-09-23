use crate::models::{ModelQuota, PlanInfo, TokenReport, UserInfo};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::{DateTime, Local, Utc};

#[derive(Debug)]
pub enum ProtoValue<'a> {
    Varint(u64),
    #[allow(dead_code)]
    Fixed64([u8; 8]),
    LengthDelimited(&'a [u8]),
    Fixed32([u8; 4]),
}

impl<'a> ProtoValue<'a> {
    pub fn as_str(&self) -> Option<&'a str> {
        match self {
            ProtoValue::LengthDelimited(bytes) => std::str::from_utf8(bytes).ok(),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&'a [u8]> {
        match self {
            ProtoValue::LengthDelimited(bytes) => Some(bytes),
            _ => None,
        }
    }

    pub fn as_varint(&self) -> Option<u64> {
        match self {
            ProtoValue::Varint(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f32> {
        match self {
            ProtoValue::Fixed32(bytes) => Some(f32::from_le_bytes(*bytes)),
            _ => None,
        }
    }
}

pub fn read_varint(data: &[u8], offset: &mut usize) -> Option<u64> {
    let mut result = 0u64;
    let mut shift = 0;
    while *offset < data.len() {
        let b = data[*offset];
        *offset += 1;
        result |= ((b & 0x7F) as u64) << shift;
        shift += 7;
        if (b & 0x80) == 0 {
            return Some(result);
        }
        if shift >= 64 {
            return None;
        }
    }
    None
}

pub fn get_fields<'a>(data: &'a [u8], target_field: u32) -> Vec<ProtoValue<'a>> {
    let mut results = Vec::new();
    let mut offset = 0;

    while offset < data.len() {
        let key = match read_varint(data, &mut offset) {
            Some(k) => k,
            None => break,
        };
        let field_num = (key >> 3) as u32;
        let wire_type = (key & 0x07) as u8;

        match wire_type {
            0 => {
                // Varint
                if let Some(v) = read_varint(data, &mut offset) {
                    if field_num == target_field {
                        results.push(ProtoValue::Varint(v));
                    }
                } else {
                    break;
                }
            }
            1 => {
                // 64-bit
                if offset + 8 <= data.len() {
                    let mut b = [0u8; 8];
                    b.copy_from_slice(&data[offset..offset + 8]);
                    offset += 8;
                    if field_num == target_field {
                        results.push(ProtoValue::Fixed64(b));
                    }
                } else {
                    break;
                }
            }
            2 => {
                // Length-delimited
                if let Some(len) = read_varint(data, &mut offset) {
                    let len = len as usize;
                    if offset + len <= data.len() {
                        let bytes = &data[offset..offset + len];
                        offset += len;
                        if field_num == target_field {
                            results.push(ProtoValue::LengthDelimited(bytes));
                        }
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            5 => {
                // 32-bit
                if offset + 4 <= data.len() {
                    let mut b = [0u8; 4];
                    b.copy_from_slice(&data[offset..offset + 4]);
                    offset += 4;
                    if field_num == target_field {
                        results.push(ProtoValue::Fixed32(b));
                    }
                } else {
                    break;
                }
            }
            _ => break,
        }
    }

    results
}

pub fn get_first_field<'a>(data: &'a [u8], target_field: u32) -> Option<ProtoValue<'a>> {
    get_fields(data, target_field).into_iter().next()
}

fn format_duration_until(target_timestamp: i64) -> String {
    let now = Utc::now().timestamp();
    let diff = target_timestamp - now;
    if diff <= 0 {
        return "Now / Expired".to_string();
    }

    let hours = diff / 3600;
    let minutes = (diff % 3600) / 60;

    if hours >= 24 {
        let days = hours / 24;
        let rem_hours = hours % 24;
        format!("{}d {}h", days, rem_hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes.max(1))
    }
}

pub fn parse_user_status(raw_val: &str, selected_model_id: Option<u32>) -> Result<TokenReport, String> {
    let d1 = BASE64.decode(raw_val.trim()).map_err(|e| format!("Base64 decode outer error: {}", e))?;

    let f1 = get_first_field(&d1, 1).and_then(|v| v.as_bytes().map(|b| b.to_vec())).ok_or("Field 1 missing")?;
    let f2 = get_first_field(&f1, 2).and_then(|v| v.as_bytes().map(|b| b.to_vec())).ok_or("Field 1.2 missing")?;
    let inner_b64 = get_first_field(&f2, 1).and_then(|v| v.as_str().map(|s| s.to_string())).ok_or("Inner base64 string missing")?;

    let inner = BASE64.decode(inner_b64.trim()).map_err(|e| format!("Base64 decode inner error: {}", e))?;

    // User info
    let user_name = get_first_field(&inner, 3).and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_default();
    let user_email = get_first_field(&inner, 7).and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_default();
    let profile_url = get_first_field(&inner, 38).and_then(|v| v.as_str().map(|s| s.to_string()));

    let user = if !user_name.is_empty() || !user_email.is_empty() {
        Some(UserInfo {
            name: user_name,
            email: user_email,
            profile_url,
        })
    } else {
        None
    };

    // Plan info
    let plan = if let Some(p_bytes) = get_first_field(&inner, 36).and_then(|v| v.as_bytes()) {
        let p_id = get_first_field(p_bytes, 1).and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_default();
        let p_name = get_first_field(p_bytes, 2).and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_default();
        let p_desc = get_first_field(p_bytes, 3).and_then(|v| v.as_str().map(|s| s.to_string()));
        Some(PlanInfo {
            id: p_id,
            name: p_name,
            description: p_desc,
        })
    } else {
        None
    };

    // Models
    let mut models = Vec::new();
    let mut selected_model_name = None;

    if let Some(f33_bytes) = get_first_field(&inner, 33).and_then(|v| v.as_bytes()) {
        let model_entries = get_fields(f33_bytes, 1);
        for m_val in model_entries {
            if let Some(m_bytes) = m_val.as_bytes() {
                let name = match get_first_field(m_bytes, 1).and_then(|v| v.as_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };

                // ID from field 2 (nested varint in field 1)
                let id = get_first_field(m_bytes, 2)
                    .and_then(|v| v.as_bytes())
                    .and_then(|b| get_first_field(b, 1))
                    .and_then(|v| v.as_varint())
                    .unwrap_or(0) as u32;

                let badge = get_first_field(m_bytes, 16).and_then(|v| v.as_str().map(|s| s.to_string()));

                // Quota info in field 15
                let mut remaining_pct = 0.0f32;
                let mut reset_timestamp = None;
                let mut reset_time = None;
                let mut time_until_reset = None;

                if let Some(f15_bytes) = get_first_field(m_bytes, 15).and_then(|v| v.as_bytes()) {
                    if let Some(fraction) = get_first_field(f15_bytes, 1).and_then(|v| v.as_float()) {
                        remaining_pct = (fraction * 100.0).clamp(0.0, 100.0);
                    }
                    if let Some(f15_sub) = get_first_field(f15_bytes, 2).and_then(|v| v.as_bytes()) {
                        if let Some(ts) = get_first_field(f15_sub, 1).and_then(|v| v.as_varint()) {
                            let ts_i64 = ts as i64;
                            reset_timestamp = Some(ts_i64);
                            if let Some(dt) = DateTime::from_timestamp(ts_i64, 0) {
                                let local_dt: DateTime<Local> = DateTime::from(dt);
                                reset_time = Some(local_dt.format("%Y-%m-%d %H:%M:%S").to_string());
                                time_until_reset = Some(format_duration_until(ts_i64));
                            }
                        }
                    }
                }

                let used_pct = (100.0 - remaining_pct).clamp(0.0, 100.0);
                let is_selected = selected_model_id.map_or(false, |sel_id| sel_id == id);
                if is_selected {
                    selected_model_name = Some(name.clone());
                }

                models.push(ModelQuota {
                    name,
                    id,
                    remaining_percentage: (remaining_pct * 10.0).round() / 10.0,
                    used_percentage: (used_pct * 10.0).round() / 10.0,
                    reset_timestamp,
                    reset_time,
                    time_until_reset,
                    badge,
                    is_selected,
                });
            }
        }
    }

    Ok(TokenReport {
        user,
        plan,
        selected_model_id,
        selected_model_name,
        models,
        fetched_at: Utc::now().timestamp(),
    })
}
