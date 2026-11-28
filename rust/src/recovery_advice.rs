//! Read-only validation of pending journal record structure.
//! A structurally valid record is untrusted evidence, not a recovery command.
pub mod transfer_evidence;

use serde_json::Value;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Advice {
    InspectTransfer,
    InspectRetainedMove,
    InspectCreation,
    InspectTrashOrRestore,
    InspectPurge,
    UnrecognizedOperation,
    MalformedRecord,
}

fn id_valid(id: &str) -> bool {
    let mut parts = id.split('-');
    for length in [32, 8, 16] {
        let Some(part) = parts.next() else {
            return false;
        };
        if part.len() != length || !part.bytes().all(|b| b.is_ascii_hexdigit()) {
            return false;
        }
    }
    parts.next().is_none()
}

/// Validate that raw and optional UTF-8 representations agree. Return presence
/// only, deliberately never expose a decoded path for subsequent actions.
fn path_present(record: &Value, key: &str) -> Option<bool> {
    let raw = record.get(format!("{key}_bytes_hex"))?;
    let text = record.get(key)?;
    if raw.is_null() {
        return text.is_null().then_some(false);
    }
    let hex = raw.as_str()?;
    if hex.is_empty() || hex.len() > 131_072 || hex.len() % 2 != 0 {
        return None;
    }
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let bytes: Vec<_> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect::<Option<_>>()?;
    if bytes.first() != Some(&b'/') || bytes.contains(&0) {
        return None;
    }
    match std::str::from_utf8(&bytes) {
        Ok(value) if text.as_str() == Some(value) => Some(true),
        Err(_) if text.is_null() => Some(true),
        _ => None,
    }
}

/// Classify the current journal schema without inspecting or mutating any
/// recorded filesystem path. All classes require independent manual evidence;
/// a pending record does not establish whether the operation completed.
pub fn classify(filename: &OsStr, record: &Value) -> Advice {
    let Some(id) = record.get("id").and_then(Value::as_str) else {
        return Advice::MalformedRecord;
    };
    if !id_valid(id)
        || filename.as_bytes() != format!("{id}.pending.json").as_bytes()
        || record.get("started").and_then(Value::as_str) != Some(id)
    {
        return Advice::MalformedRecord;
    }
    let (Some(source), Some(destination), Some(stage)) = (
        path_present(record, "source"),
        path_present(record, "destination"),
        path_present(record, "stage"),
    ) else {
        return Advice::MalformedRecord;
    };
    match record.get("operation").and_then(Value::as_str) {
        Some("copy" | "copy-time-tolerance") if source && destination && stage => Advice::InspectTransfer,
        Some("retained-move" | "retained-move-time-tolerance") if source && destination && stage => {
            Advice::InspectRetainedMove
        }
        Some("move") if source && destination && !stage => Advice::InspectTransfer,
        Some("archive-extract") if !source && destination && stage => Advice::InspectCreation,
        Some("mkdir" | "create-file") if !source && destination && !stage => {
            Advice::InspectCreation
        }
        Some("native-trash" | "restore") if source && destination && stage => {
            Advice::InspectTrashOrRestore
        }
        Some("permanent-delete") if source && !destination => Advice::InspectPurge,
        Some(
            "copy" | "move" | "mkdir" | "create-file" | "native-trash" | "restore"
            | "permanent-delete" | "archive-extract" | "retained-move"
            | "copy-time-tolerance" | "retained-move-time-tolerance",
        )
        | None => Advice::MalformedRecord,
        Some(_) => Advice::UnrecognizedOperation,
    }
}

impl Advice {
    pub fn message(self) -> &'static str {
        match self {
            Self::InspectTransfer => {
                "Inspect source, destination and any staging data; do not retry blindly."
            }
            Self::InspectRetainedMove => {
                "Retained move: preserve all copies. Inspect the original path, destination and retained original (stage). If the original is absent, independently verify the retained file and copy it to a new unoccupied recovery path. Never overwrite, purge or replay from journal evidence."
            }
            Self::InspectCreation => {
                "Inspect the destination and any staging data; verify identity before any retry."
            }
            Self::InspectTrashOrRestore => {
                "Inspect source, destination and Trash metadata together."
            }
            Self::InspectPurge => {
                "Inspect the original item and metadata; do not repeat deletion blindly."
            }
            Self::UnrecognizedOperation => {
                "Unrecognized operation; preserve evidence and review manually."
            }
            Self::MalformedRecord => {
                "Malformed journal evidence; preserve it and do not use recorded paths for actions."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "00000000000000000000000000000001-00000002-0000000000000003";

    fn copy_record() -> Value {
        json!({
            "id": ID, "started": ID, "operation": "copy",
            "source": "/a", "source_bytes_hex": "2f61",
            "destination": "/b", "destination_bytes_hex": "2f62",
            "stage": "/s", "stage_bytes_hex": "2f73",
        })
    }

    fn inspect(record: &Value) -> Advice {
        classify(OsStr::new(&format!("{ID}.pending.json")), record)
    }

    #[test]
    fn known_operations_have_only_advisory_classes() {
        let mut record = copy_record();
        assert_eq!(inspect(&record), Advice::InspectTransfer);
        for operation in ["retained-move", "retained-move-time-tolerance"] {
            record["operation"] = json!(operation);
            assert_eq!(inspect(&record), Advice::InspectRetainedMove);
        }
        record["operation"] = json!("copy-time-tolerance");
        assert_eq!(inspect(&record), Advice::InspectTransfer);
        record["operation"] = json!("restore");
        assert_eq!(inspect(&record), Advice::InspectTrashOrRestore);
        record["operation"] = json!("move");
        record["stage"] = Value::Null;
        record["stage_bytes_hex"] = Value::Null;
        assert_eq!(inspect(&record), Advice::InspectTransfer);
        record["operation"] = json!("mkdir");
        record["source"] = Value::Null;
        record["source_bytes_hex"] = Value::Null;
        assert_eq!(inspect(&record), Advice::InspectCreation);
        record["operation"] = json!("permanent-delete");
        record["source"] = json!("/a");
        record["source_bytes_hex"] = json!("2f61");
        record["destination"] = Value::Null;
        record["destination_bytes_hex"] = Value::Null;
        assert_eq!(inspect(&record), Advice::InspectPurge);
    }

    #[test]
    fn invalid_or_conflicting_path_encoding_is_refused() {
        for hex in ["2f00", "zz", "2", "6162", "2f63"] {
            let mut record = copy_record();
            record["source_bytes_hex"] = json!(hex);
            assert_eq!(inspect(&record), Advice::MalformedRecord);
        }
        let mut record = copy_record();
        record["source"] = Value::Null;
        record["source_bytes_hex"] = json!("2fff");
        assert_eq!(inspect(&record), Advice::InspectTransfer);
    }

    #[test]
    fn incorrect_filename_id_or_field_shape_is_refused() {
        let mut record = copy_record();
        assert_eq!(
            classify(OsStr::new("other.pending.json"), &record),
            Advice::MalformedRecord
        );
        record["started"] = json!("wrong");
        assert_eq!(inspect(&record), Advice::MalformedRecord);
        let mut record = copy_record();
        record["stage"] = Value::Null;
        record["stage_bytes_hex"] = Value::Null;
        assert_eq!(inspect(&record), Advice::MalformedRecord);
    }

    #[test]
    fn archive_extraction_evidence_requires_destination_and_stage() {
        let mut record = copy_record();
        record["operation"] = json!("archive-extract");
        assert_eq!(inspect(&record), Advice::MalformedRecord);
        record["source"] = Value::Null;
        record["source_bytes_hex"] = Value::Null;
        assert_eq!(inspect(&record), Advice::InspectCreation);
        record["stage"] = Value::Null;
        record["stage_bytes_hex"] = Value::Null;
        assert_eq!(inspect(&record), Advice::MalformedRecord);
    }

    #[test]
    fn unknown_operations_never_imply_a_replay() {
        let mut record = copy_record();
        record["operation"] = json!("future-operation");
        assert_eq!(inspect(&record), Advice::UnrecognizedOperation);
    }
}
