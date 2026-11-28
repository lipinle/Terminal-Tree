//! Structural validation of ordered transfer evidence, not recovery authority.
//! A record can be forged or stale. No filesystem access or mutation here.
use super::id_valid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Intent,
    StageVerified,
    DestinationPublished,
    SourceRetained,
    RetentionVerified,
}

const ORDER: [Phase; 5] = [
    Phase::Intent,
    Phase::StageVerified,
    Phase::DestinationPublished,
    Phase::SourceRetained,
    Phase::RetentionVerified,
];

#[derive(Clone, Copy, Debug)]
pub struct Record<'a> {
    pub version: u32,
    pub transaction: &'a str,
    pub sequence: u32,
    pub phase: Phase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evidence {
    Missing,
    Malformed,
    RecordedThrough(Phase),
}

/// Accept only a nonempty, contiguous prefix of the known sequence for one
/// transaction. This checks structure only, not durability, identity, content,
/// signatures or the actual state of any source/destination/retention path.
/// Even a final phase never authorizes deletion, replay or rollback.
pub fn inspect(transaction: &str, records: &[Record<'_>]) -> Evidence {
    if !id_valid(transaction) || records.len() > ORDER.len() {
        return Evidence::Malformed;
    }
    for (index, record) in records.iter().enumerate() {
        if record.version != 1
            || record.transaction != transaction
            || record.sequence != index as u32
            || record.phase != ORDER[index]
        {
            return Evidence::Malformed;
        }
    }
    records.last().map_or(Evidence::Missing, |record| {
        Evidence::RecordedThrough(record.phase)
    })
}

// Fixed, bounded ASCII evidence records for read-only inspection. This is a
// proposed v1 encoding, not a persisted journal or a durability guarantee.
const MAX_ENCODED_BYTES: usize = 1024;

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Intent => "intent",
        Phase::StageVerified => "stage-verified",
        Phase::DestinationPublished => "destination-published",
        Phase::SourceRetained => "source-retained",
        Phase::RetentionVerified => "retention-verified",
    }
}

/// Canonically encode an already structurally valid sequence. No I/O.
pub fn encode(transaction: &str, records: &[Record<'_>]) -> Option<Vec<u8>> {
    if inspect(transaction, records) == Evidence::Malformed {
        return None;
    }
    let mut text = String::new();
    for record in records {
        use std::fmt::Write;
        writeln!(
            text,
            "1\t{}\t{}\t{}",
            record.transaction,
            record.sequence,
            phase_name(record.phase)
        )
        .ok()?;
    }
    (text.len() <= MAX_ENCODED_BYTES).then(|| text.into_bytes())
}

/// Inspect a complete sequence of canonical newline-terminated records.
/// Reject torn final records, extra fields, unknown versions, and excess data.
/// A structurally valid prefix is not proof of actual filesystem state.
pub fn inspect_encoded(transaction: &str, bytes: &[u8]) -> Evidence {
    if !id_valid(transaction) || bytes.len() > MAX_ENCODED_BYTES {
        return Evidence::Malformed;
    }
    if bytes.is_empty() {
        return Evidence::Missing;
    }
    if !bytes.ends_with(b"\n") {
        return Evidence::Malformed;
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Evidence::Malformed;
    };
    let mut records = Vec::new();
    for (index, line) in text.split_terminator('\n').enumerate() {
        let Some(&phase) = ORDER.get(index) else {
            return Evidence::Malformed;
        };
        let expected = format!("1\t{transaction}\t{index}\t{}", phase_name(phase));
        if line != expected {
            return Evidence::Malformed;
        }
        records.push(Record {
            version: 1,
            transaction,
            sequence: index as u32,
            phase,
        });
    }
    inspect(transaction, &records)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "00000000000000000000000000000001-00000002-0000000000000003";

    fn records() -> Vec<Record<'static>> {
        ORDER
            .iter()
            .enumerate()
            .map(|(index, &phase)| Record {
                version: 1,
                transaction: ID,
                sequence: index as u32,
                phase,
            })
            .collect()
    }

    #[test]
    fn every_interruption_prefix_is_only_recorded_evidence() {
        let all = records();
        assert_eq!(inspect(ID, &[]), Evidence::Missing);
        for (index, &phase) in ORDER.iter().enumerate() {
            assert_eq!(
                inspect(ID, &all[..=index]),
                Evidence::RecordedThrough(phase)
            );
        }
    }

    #[test]
    fn canonical_encoding_round_trips_every_prefix() {
        let all = records();
        for count in 0..=all.len() {
            let encoded = encode(ID, &all[..count]).unwrap();
            assert_eq!(inspect_encoded(ID, &encoded), inspect(ID, &all[..count]));
            if count != 0 {
                assert_eq!(
                    inspect_encoded(ID, &encoded[..encoded.len() - 1]),
                    Evidence::Malformed
                );
            }
        }
    }

    #[test]
    fn encoded_evidence_rejects_noncanonical_and_excess_records() {
        let all = records();
        let encoded = encode(ID, &all).unwrap();
        let mut extra = encoded.clone();
        extra.extend_from_slice(b"\n");
        assert_eq!(inspect_encoded(ID, &extra), Evidence::Malformed);
        for bad in [
            format!("2\t{ID}\t0\tintent\n"),
            format!("1\t{ID}\t00\tintent\n"),
            format!("1\t{ID}\t0\tintent\textra\n"),
            format!("1\t{ID}\t1\tstage-verified\n"),
        ] {
            assert_eq!(inspect_encoded(ID, bad.as_bytes()), Evidence::Malformed);
        }
        assert_eq!(inspect_encoded(ID, &[b'x'; 1025]), Evidence::Malformed);
        assert_eq!(inspect_encoded(ID, &[0xff, b'\n']), Evidence::Malformed);
        assert!(encode(ID, &all[1..]).is_none());
    }

    #[test]
    fn rejects_gaps_reordering_duplicates_and_unknown_versions() {
        let all = records();
        let mut broken = all.clone();
        broken.remove(1);
        assert_eq!(inspect(ID, &broken), Evidence::Malformed);
        let mut broken = all.clone();
        broken.swap(1, 2);
        assert_eq!(inspect(ID, &broken), Evidence::Malformed);
        let mut broken = all.clone();
        broken[2] = broken[1];
        assert_eq!(inspect(ID, &broken), Evidence::Malformed);
        let mut broken = all.clone();
        broken[0].version = 2;
        assert_eq!(inspect(ID, &broken), Evidence::Malformed);
        let mut broken = all.clone();
        broken[3].transaction = "other";
        assert_eq!(inspect(ID, &broken), Evidence::Malformed);
        let mut broken = all.clone();
        broken.push(all[4]);
        assert_eq!(inspect(ID, &broken), Evidence::Malformed);
        assert_eq!(inspect("invalid", &[]), Evidence::Malformed);
    }
}
