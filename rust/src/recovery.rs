//! Display only: journal paths are untrusted data, never action inputs.
use serde_json::Value;
use std::{ffi::OsString, os::unix::ffi::OsStringExt};
use terminal_tree::{display_name, recovery_advice::classify};

fn field(record: &Value, key: &str) -> String {
    let hexkey = format!("{key}_bytes_hex");
    if let Some(s) = record.get(&hexkey).and_then(Value::as_str) {
        if s.len() > 131_072 {
            return "[path encoding exceeds display limit]".into();
        }
        if s.len() % 2 != 0 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return "[invalid path encoding]".into();
        }
        let bytes: Vec<_> = (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect();
        if bytes.contains(&0) {
            return "[invalid NUL path]".into();
        }
        return display_name(&OsString::from_vec(bytes));
    }
    match record.get(key).and_then(Value::as_str) {
        Some(s) if s.len() > 65_536 => "[text exceeds display limit]".into(),
        Some(s) => display_name(s.as_ref()),
        None => "-".into(),
    }
}
pub fn show(
    records: &[(OsString, std::io::Result<Value>)],
    truncated: bool,
) -> std::io::Result<()> {
    let mut lines = vec![
        format!("Pending records: {}", records.len()),
        "Pending does not mean failed. A write may already have completed.".into(),
        "Inspect original, destination and staging/metadata before retrying.".into(),
        "Read-only view; no automatic deletion, rollback or replay.".into(),
    ];
    if truncated {
        lines.push(
            "Report incomplete: scan limited to 4096 directory entries and 128 records. No records were modified."
                .into(),
        );
    }
    for (name, result) in records {
        lines.push(String::new());
        lines.push(format!("Record: {}", display_name(name)));
        let r = match result {
            Ok(r) => r,
            Err(e) => {
                lines.push(format!(
                    "Cannot read: {}",
                    display_name(OsString::from(e.to_string()).as_os_str())
                ));
                continue;
            }
        };
        lines.push(format!(
            "ID: {} | Operation: {}",
            field(r, "id"),
            field(r, "operation")
        ));
        lines.push(classify(name, r).message().into());
        for key in ["source", "destination", "stage"] {
            lines.push(format!("{key}: {}", field(r, key)));
        }
    }
    use unicode_width::UnicodeWidthChar;
    let width = usize::from(crossterm::terminal::size()?.0)
        .saturating_sub(1)
        .max(1);
    let mut wrapped = Vec::new();
    for line in lines {
        let mut part = String::new();
        let mut used = 0;
        for c in line.chars() {
            let n = c.width().unwrap_or(0);
            if used + n > width && !part.is_empty() {
                wrapped.push(std::mem::take(&mut part));
                used = 0;
            }
            part.push(c);
            used += n;
        }
        wrapped.push(part);
    }
    super::page("Recovery - pending journal", &wrapped)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_display_fields_are_refused_before_decoding() {
        let r = serde_json::json!({"source_bytes_hex": "2f".repeat(65_537)});
        assert_eq!(field(&r, "source"), "[path encoding exceeds display limit]");
        let r = serde_json::json!({"source": "x".repeat(65_537)});
        assert_eq!(field(&r, "source"), "[text exceeds display limit]");
    }

    #[test]
    fn raw_paths_and_controls_are_safe() {
        let r = serde_json::json!({"source":null,"source_bytes_hex":"2f746d702fff1b0a"});
        let s = field(&r, "source");
        assert!(s.contains("\\xff"));
        assert!(!s.contains('\x1b'));
        assert!(!s.contains('\n'));
        assert_eq!(
            field(&serde_json::json!({"source_bytes_hex":"zz"}), "source"),
            "[invalid path encoding]"
        );
    }
}
