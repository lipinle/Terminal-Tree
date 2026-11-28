//! Conservative bulk-rename preflight. A plan is not authorization to write.
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};

const MAX_ITEMS: usize = 1000;
const MAX_DIRECTORY_NAMES: usize = 16_384;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rename {
    pub source: OsString,
    pub destination: OsString,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanError {
    Limit,
    InvalidName,
    DuplicateSource,
    DuplicateDestination,
    MissingSource,
    DestinationOccupied,
}

fn single_name(name: &OsStr) -> bool {
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && !name.as_bytes().contains(&b'/')
        && !name.as_bytes().contains(&0)
}

/// Preflight one directory using an observed snapshot of all existing names.
/// Exact raw-name comparison only; filesystem-specific aliases still require
/// native checks. Reject cycles and chains into occupied names, even if those
/// names are sources elsewhere in the plan. Never execute this plan directly.
pub fn validate(existing: &[OsString], requested: &[Rename]) -> Result<(), PlanError> {
    if existing.len() > MAX_DIRECTORY_NAMES
        || requested.is_empty()
        || requested.len() > MAX_ITEMS
    {
        return Err(PlanError::Limit);
    }
    let mut occupied = BTreeSet::new();
    for name in existing {
        if !single_name(name) {
            return Err(PlanError::InvalidName);
        }
        if !occupied.insert(name.as_os_str()) {
            return Err(PlanError::DuplicateSource);
        }
    }
    let mut sources = BTreeSet::new();
    let mut destinations = BTreeSet::new();
    for item in requested {
        if !single_name(&item.source) || !single_name(&item.destination) {
            return Err(PlanError::InvalidName);
        }
        if !sources.insert(item.source.as_os_str()) {
            return Err(PlanError::DuplicateSource);
        }
        if !destinations.insert(item.destination.as_os_str()) {
            return Err(PlanError::DuplicateDestination);
        }
        if !occupied.contains(item.source.as_os_str()) {
            return Err(PlanError::MissingSource);
        }
        if occupied.contains(item.destination.as_os_str()) {
            return Err(PlanError::DestinationOccupied);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(source: &str, destination: &str) -> Rename {
        Rename {
            source: source.into(),
            destination: destination.into(),
        }
    }

    #[test]
    fn distinct_unoccupied_names_pass_preflight() {
        let existing = ["a".into(), "b".into()];
        assert_eq!(validate(&existing, &[item("a", "c"), item("b", "d")]), Ok(()));
    }

    #[test]
    fn duplicate_sources_and_destinations_are_refused() {
        let existing = ["a".into(), "b".into()];
        assert_eq!(
            validate(&existing, &[item("a", "c"), item("a", "d")]),
            Err(PlanError::DuplicateSource)
        );
        assert_eq!(
            validate(&existing, &[item("a", "c"), item("b", "c")]),
            Err(PlanError::DuplicateDestination)
        );
    }

    #[test]
    fn occupied_targets_and_missing_sources_are_refused() {
        let existing = ["a".into(), "b".into()];
        for plan in [
            vec![item("a", "b"), item("b", "a")],
            vec![item("a", "b"), item("b", "c")],
            vec![item("a", "a")],
        ] {
            assert_eq!(validate(&existing, &plan), Err(PlanError::DestinationOccupied));
        }
        assert_eq!(
            validate(&existing, &[item("missing", "c")]),
            Err(PlanError::MissingSource)
        );
    }

    #[test]
    fn raw_names_are_preserved_and_path_components_refused() {
        use std::os::unix::ffi::OsStringExt;
        let raw = OsString::from_vec(b"raw-\xff".to_vec());
        let plan = Rename {
            source: raw.clone(),
            destination: OsString::from_vec(b"new-\xfe".to_vec()),
        };
        assert_eq!(validate(&[raw], &[plan]), Ok(()));
        for name in ["", ".", "..", "a/b", "a/", "/absolute", "a\0b"] {
            assert_eq!(
                validate(&["a".into()], &[item("a", name)]),
                Err(PlanError::InvalidName)
            );
        }
    }

    #[test]
    fn oversized_or_empty_plans_are_refused() {
        assert_eq!(validate(&[], &[]), Err(PlanError::Limit));
        let large = vec![item("a", "b"); MAX_ITEMS + 1];
        assert_eq!(validate(&["a".into()], &large), Err(PlanError::Limit));
    }
}
