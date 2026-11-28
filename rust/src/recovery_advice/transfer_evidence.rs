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
