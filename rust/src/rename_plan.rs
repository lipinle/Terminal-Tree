//! Conservative bulk-rename preflight. A plan is not authorization to write.
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path};

pub mod native;

const MAX_ITEMS: usize = 1000;
const MAX_DIRECTORY_NAMES: usize = 16_384;
const MAX_NAME_BYTES: usize = 255;

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
    InvalidPattern,
    Unchanged,
}

fn single_name(name: &OsStr) -> bool {
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && name.as_bytes().len() <= MAX_NAME_BYTES
        && !name.as_bytes().contains(&b'/')
        && !name.as_bytes().contains(&0)
}

/// Preflight one directory using an observed snapshot of all existing names.
/// Exact raw-name comparison only; filesystem-specific aliases still require
/// native checks. Reject cycles and chains into occupied names, even if those
/// names are sources elsewhere in the plan. Never execute this plan directly.
pub fn validate(existing: &[OsString], requested: &[Rename]) -> Result<(), PlanError> {
    if existing.len() > MAX_DIRECTORY_NAMES || requested.is_empty() || requested.len() > MAX_ITEMS {
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

/// Build a preview-only plan for literal, non-overlapping byte replacement
/// within selected filenames. Preserve native names without UTF-8 conversion.
/// Every selected name must change; an error discards the entire plan.
/// Never use a successful plan as filesystem authorization.
pub fn replace_literal(
    existing: &[OsString],
    selected: &[OsString],
    needle: &OsStr,
    replacement: &OsStr,
) -> Result<Vec<Rename>, PlanError> {
    let needle = needle.as_bytes();
    let replacement = replacement.as_bytes();
    if selected.is_empty() || selected.len() > MAX_ITEMS || existing.len() > MAX_DIRECTORY_NAMES {
        return Err(PlanError::Limit);
    }
    if needle.is_empty()
        || needle.len() > MAX_NAME_BYTES
        || replacement.len() > MAX_NAME_BYTES
        || needle
            .iter()
            .chain(replacement)
            .any(|&b| b == 0 || b == b'/')
    {
        return Err(PlanError::InvalidPattern);
    }
    let mut plan = Vec::with_capacity(selected.len());
    for source in selected {
        if !single_name(source) {
            return Err(PlanError::InvalidName);
        }
        let raw = source.as_bytes();
        let mut destination = Vec::with_capacity(raw.len());
        let mut offset = 0;
        while offset < raw.len() {
            let chunk = if raw[offset..].starts_with(needle) {
                offset += needle.len();
                replacement
            } else {
                let start = offset;
                offset += 1;
                &raw[start..offset]
            };
            if destination.len() + chunk.len() > MAX_NAME_BYTES {
                return Err(PlanError::InvalidName);
            }
            destination.extend_from_slice(chunk);
        }
        if destination == raw {
            return Err(PlanError::Unchanged);
        }
        plan.push(Rename {
            source: source.clone(),
            destination: OsString::from_vec(destination),
        });
    }
    validate(existing, &plan)?;
    Ok(plan)
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
        assert_eq!(
            validate(&existing, &[item("a", "c"), item("b", "d")]),
            Ok(())
        );
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
            assert_eq!(
                validate(&existing, &plan),
                Err(PlanError::DestinationOccupied)
            );
        }
        assert_eq!(
            validate(&existing, &[item("missing", "c")]),
            Err(PlanError::MissingSource)
        );
    }

    #[test]
    fn raw_names_are_preserved_and_path_components_refused() {
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

    #[test]
    fn literal_replacement_preserves_order_and_raw_bytes() {
        let existing = [
            OsString::from_vec(b"old-\xff-old".to_vec()),
            OsString::from("old-file"),
        ];
        let result =
            replace_literal(&existing, &existing, OsStr::new("old"), OsStr::new("new")).unwrap();
        assert_eq!(result[0].source, existing[0]);
        assert_eq!(result[0].destination.as_bytes(), b"new-\xff-new");
        assert_eq!(result[1], item("old-file", "new-file"));
        assert_eq!(existing[1], "old-file");
    }

    #[test]
    fn literal_replacement_rejects_entire_conflicting_plan() {
        let existing = ["old-a".into(), "old-b".into(), "new-b".into()];
        assert_eq!(
            replace_literal(
                &existing,
                &existing[..2],
                OsStr::new("old"),
                OsStr::new("new")
            ),
            Err(PlanError::DestinationOccupied)
        );
        assert_eq!(
            replace_literal(&existing, &existing, OsStr::new("old"), OsStr::new("new")),
            Err(PlanError::Unchanged)
        );
        let names = ["a-x".into(), "x-a".into()];
        assert_eq!(
            replace_literal(&names, &names, OsStr::new("x"), OsStr::new("a")),
            Err(PlanError::DuplicateDestination)
        );
    }

    #[test]
    fn literal_replacement_rejects_invalid_patterns_and_names() {
        let names = ["a".into()];
        for needle in ["", "/", "\0"] {
            assert_eq!(
                replace_literal(&names, &names, OsStr::new(needle), OsStr::new("b")),
                Err(PlanError::InvalidPattern)
            );
        }
        for replacement in ["", ".", ".."] {
            assert_eq!(
                replace_literal(&names, &names, OsStr::new("a"), OsStr::new(replacement)),
                Err(PlanError::InvalidName)
            );
        }
        assert_eq!(
            replace_literal(&names, &names, OsStr::new("a"), OsStr::new("a")),
            Err(PlanError::Unchanged)
        );
        assert_eq!(
            replace_literal(&names, &names, OsStr::new("a"), OsStr::new("b/c")),
            Err(PlanError::InvalidPattern)
        );
        let long = OsString::from("a".repeat(MAX_NAME_BYTES));
        assert_eq!(
            replace_literal(
                std::slice::from_ref(&long),
                std::slice::from_ref(&long),
                OsStr::new("a"),
                OsStr::new("bb")
            ),
            Err(PlanError::InvalidName)
        );
    }
}
