use crate::models::{ModelQuota, PlanInfo, Provider, TokenReport, UserInfo};
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
                    // Checked, because a corrupt length near usize::MAX would overflow.
                    let end = usize::try_from(len).ok().and_then(|len| offset.checked_add(len));
                    if let Some(end) = end.filter(|&end| end <= data.len()) {
                        let bytes = &data[offset..end];
                        offset = end;
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

pub fn format_duration_until(target_timestamp: i64) -> String {
    format_duration_between(target_timestamp, Utc::now().timestamp())
}

fn format_duration_between(target_timestamp: i64, now: i64) -> String {
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
                let is_selected = selected_model_id.is_some_and(|sel_id| sel_id == id);
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
        provider: Provider::Antigravity,
        user,
        plan,
        selected_model_id,
        selected_model_name,
        models,
        fetched_at: Utc::now().timestamp(),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // Minimal protobuf encoder, so tests can build messages field by field.
    pub(crate) fn varint(mut v: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (v & 0x7F) as u8;
            v >>= 7;
            if v == 0 {
                out.push(byte);
                return out;
            }
            out.push(byte | 0x80);
        }
    }

    fn key(field: u32, wire_type: u8) -> Vec<u8> {
        varint(((field as u64) << 3) | wire_type as u64)
    }

    pub(crate) fn field_varint(field: u32, v: u64) -> Vec<u8> {
        [key(field, 0), varint(v)].concat()
    }

    pub(crate) fn field_bytes(field: u32, bytes: &[u8]) -> Vec<u8> {
        [key(field, 2), varint(bytes.len() as u64), bytes.to_vec()].concat()
    }

    fn field_f32(field: u32, v: f32) -> Vec<u8> {
        [key(field, 5), v.to_le_bytes().to_vec()].concat()
    }

    fn field_f64(field: u32, v: [u8; 8]) -> Vec<u8> {
        [key(field, 1), v.to_vec()].concat()
    }

    // --- read_varint ---

    #[test]
    fn varint_round_trips() {
        for v in [0, 1, 127, 128, 300, u32::MAX as u64, u64::MAX] {
            let bytes = varint(v);
            let mut offset = 0;
            assert_eq!(read_varint(&bytes, &mut offset), Some(v), "value {v}");
            assert_eq!(offset, bytes.len());
        }
    }

    #[test]
    fn varint_truncated_is_none() {
        let mut offset = 0;
        assert_eq!(read_varint(&[0x80, 0x80], &mut offset), None);
        let mut offset = 0;
        assert_eq!(read_varint(&[], &mut offset), None);
    }

    #[test]
    fn varint_longer_than_ten_bytes_is_none() {
        let mut offset = 0;
        assert_eq!(read_varint(&[0xFF; 11], &mut offset), None);
    }

    // --- get_fields ---

    #[test]
    fn get_fields_reads_every_wire_type() {
        let data = [
            field_varint(1, 150),
            field_bytes(2, b"hi"),
            field_f32(3, 0.5),
            field_f64(4, [1, 2, 3, 4, 5, 6, 7, 8]),
        ]
        .concat();

        assert_eq!(get_first_field(&data, 1).and_then(|v| v.as_varint()), Some(150));
        assert_eq!(get_first_field(&data, 2).and_then(|v| v.as_str()), Some("hi"));
        assert_eq!(get_first_field(&data, 3).and_then(|v| v.as_float()), Some(0.5));
        assert!(matches!(get_first_field(&data, 4), Some(ProtoValue::Fixed64([1, 2, 3, 4, 5, 6, 7, 8]))));
        assert!(get_first_field(&data, 5).is_none());
    }

    #[test]
    fn get_fields_returns_repeated_fields_in_order() {
        let data = [field_bytes(1, b"a"), field_varint(2, 9), field_bytes(1, b"b")].concat();
        let values: Vec<_> = get_fields(&data, 1).iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(values, ["a", "b"]);
    }

    #[test]
    fn accessors_reject_the_wrong_type() {
        let data = field_varint(1, 5);
        let value = get_first_field(&data, 1).unwrap();
        assert_eq!(value.as_str(), None);
        assert_eq!(value.as_bytes(), None);
        assert_eq!(value.as_float(), None);

        let invalid_utf8 = field_bytes(1, &[0xFF, 0xFE]);
        assert_eq!(get_first_field(&invalid_utf8, 1).and_then(|v| v.as_str()), None);
    }

    #[test]
    fn get_fields_stops_at_truncated_data_but_keeps_earlier_fields() {
        let good = field_varint(1, 7);
        let cases: [Vec<u8>; 4] = [
            [good.clone(), key(1, 2), varint(10), b"short".to_vec()].concat(),
            [good.clone(), key(1, 5), vec![0, 0]].concat(),
            [good.clone(), key(1, 1), vec![0; 7]].concat(),
            [good.clone(), key(1, 0), vec![0x80]].concat(),
        ];
        for data in cases {
            let found: Vec<_> = get_fields(&data, 1).iter().filter_map(|v| v.as_varint()).collect();
            assert_eq!(found, [7]);
        }
    }

    #[test]
    fn get_fields_stops_at_unknown_wire_types() {
        // Wire types 3 and 4 (deprecated groups) and 6/7 are not supported.
        for wire_type in [3, 4, 6, 7] {
            let data = [field_varint(1, 7), key(1, wire_type), field_varint(1, 8)].concat();
            assert_eq!(get_fields(&data, 1).len(), 1, "wire type {wire_type}");
        }
    }

    #[test]
    fn get_fields_survives_an_oversized_length() {
        // A length near usize::MAX must not overflow when added to the offset.
        let data = [field_varint(1, 7), key(2, 2), varint(u64::MAX), b"x".to_vec()].concat();
        assert_eq!(get_fields(&data, 1).len(), 1);
        assert!(get_fields(&data, 2).is_empty());
    }

    #[test]
    fn get_fields_on_empty_input() {
        assert!(get_fields(&[], 1).is_empty());
    }

    // --- format_duration_between ---

    #[test]
    fn durations_format_by_size() {
        let now = 1_000_000;
        assert_eq!(format_duration_between(now, now), "Now / Expired");
        assert_eq!(format_duration_between(now - 60, now), "Now / Expired");
        assert_eq!(format_duration_between(now + 10, now), "1m");
        assert_eq!(format_duration_between(now + 59 * 60, now), "59m");
        assert_eq!(format_duration_between(now + 3600 + 5 * 60, now), "1h 5m");
        assert_eq!(format_duration_between(now + 23 * 3600 + 59 * 60, now), "23h 59m");
        assert_eq!(format_duration_between(now + 24 * 3600, now), "1d 0h");
        assert_eq!(format_duration_between(now + 50 * 3600, now), "2d 2h");
    }

    // --- parse_user_status ---

    fn model(name: &str, id: u64, fraction: f32, reset: Option<u64>, badge: Option<&str>) -> Vec<u8> {
        let mut quota = field_f32(1, fraction);
        if let Some(ts) = reset {
            quota.extend(field_bytes(2, &field_varint(1, ts)));
        }
        let mut m = [
            field_bytes(1, name.as_bytes()),
            field_bytes(2, &field_varint(1, id)),
            field_bytes(15, &quota),
        ]
        .concat();
        if let Some(b) = badge {
            m.extend(field_bytes(16, b.as_bytes()));
        }
        m
    }

    /// Wraps an inner user-status message the way state.vscdb stores it:
    /// base64(1 { 2 { 1: base64(inner) } }).
    fn wrap_user_status(inner: &[u8]) -> String {
        let inner_b64 = BASE64.encode(inner);
        let f2 = field_bytes(1, inner_b64.as_bytes());
        let f1 = field_bytes(2, &f2);
        BASE64.encode(field_bytes(1, &f1))
    }

    fn sample_inner() -> Vec<u8> {
        let models = [
            field_bytes(1, &model("Gemini Flash", 10, 0.72, Some(4_000_000_000), Some("New"))),
            field_bytes(1, &model("Claude Sonnet", 20, 1.0, None, None)),
            // A model entry without a name is skipped.
            field_bytes(1, &field_bytes(2, &field_varint(1, 30))),
        ]
        .concat();
        [
            field_bytes(3, b"Ada"),
            field_bytes(7, b"ada@example.com"),
            field_bytes(38, b"https://example.com/ada.png"),
            field_bytes(36, &[field_bytes(1, b"pro"), field_bytes(2, b"Pro Plan"), field_bytes(3, b"Paid")].concat()),
            field_bytes(33, &models),
        ]
        .concat()
    }

    #[test]
    fn parses_a_full_user_status() {
        let report = parse_user_status(&wrap_user_status(&sample_inner()), Some(20)).unwrap();

        assert_eq!(report.provider, Provider::Antigravity);
        let user = report.user.unwrap();
        assert_eq!((user.name.as_str(), user.email.as_str()), ("Ada", "ada@example.com"));
        assert_eq!(user.profile_url.as_deref(), Some("https://example.com/ada.png"));

        let plan = report.plan.unwrap();
        assert_eq!((plan.id.as_str(), plan.name.as_str()), ("pro", "Pro Plan"));
        assert_eq!(plan.description.as_deref(), Some("Paid"));

        assert_eq!(report.models.len(), 2);
        let flash = &report.models[0];
        assert_eq!((flash.name.as_str(), flash.id), ("Gemini Flash", 10));
        assert_eq!(flash.remaining_percentage, 72.0);
        assert_eq!(flash.used_percentage, 28.0);
        assert_eq!(flash.reset_timestamp, Some(4_000_000_000));
        assert!(flash.reset_time.is_some() && flash.time_until_reset.is_some());
        assert_eq!(flash.badge.as_deref(), Some("New"));
        assert!(!flash.is_selected);

        let sonnet = &report.models[1];
        assert_eq!(sonnet.remaining_percentage, 100.0);
        assert_eq!(sonnet.reset_timestamp, None);
        assert!(sonnet.is_selected);
        assert_eq!(report.selected_model_id, Some(20));
        assert_eq!(report.selected_model_name.as_deref(), Some("Claude Sonnet"));
    }

    #[test]
    fn missing_sections_are_optional() {
        let report = parse_user_status(&wrap_user_status(&[]), None).unwrap();
        assert!(report.user.is_none());
        assert!(report.plan.is_none());
        assert!(report.models.is_empty());
        assert!(report.selected_model_name.is_none());
    }

    #[test]
    fn remaining_fraction_is_clamped_and_rounded() {
        let models = [
            field_bytes(1, &model("Over", 1, 1.5, None, None)),
            field_bytes(1, &model("Under", 2, -0.2, None, None)),
            field_bytes(1, &model("Fine", 3, 0.12345, None, None)),
        ]
        .concat();
        let report = parse_user_status(&wrap_user_status(&field_bytes(33, &models)), None).unwrap();
        let pcts: Vec<f32> = report.models.iter().map(|m| m.remaining_percentage).collect();
        assert_eq!(pcts, [100.0, 0.0, 12.3]);
    }

    #[test]
    fn unknown_selected_id_selects_nothing() {
        let report = parse_user_status(&wrap_user_status(&sample_inner()), Some(999)).unwrap();
        assert!(report.models.iter().all(|m| !m.is_selected));
        assert!(report.selected_model_name.is_none());
    }

    #[test]
    fn malformed_user_status_is_an_error() {
        assert!(parse_user_status("not base64!", None).unwrap_err().contains("outer"));
        assert_eq!(parse_user_status(&BASE64.encode(b""), None).unwrap_err(), "Field 1 missing");
        let no_f2 = BASE64.encode(field_bytes(1, &field_varint(9, 1)));
        assert_eq!(parse_user_status(&no_f2, None).unwrap_err(), "Field 1.2 missing");
        let no_inner = BASE64.encode(field_bytes(1, &field_bytes(2, &field_varint(9, 1))));
        assert_eq!(parse_user_status(&no_inner, None).unwrap_err(), "Inner base64 string missing");
        let bad_inner = BASE64.encode(field_bytes(1, &field_bytes(2, &field_bytes(1, b"@@@"))));
        assert!(parse_user_status(&bad_inner, None).unwrap_err().contains("inner"));
    }

    #[test]
    fn surrounding_whitespace_is_ignored() {
        let raw = format!("  {}\n", wrap_user_status(&sample_inner()));
        assert!(parse_user_status(&raw, None).is_ok());
    }
}
