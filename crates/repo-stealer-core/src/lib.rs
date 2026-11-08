//! Read-only planning primitives. No network, persistence, or Git execution.
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefSnapshot(BTreeMap<String, String>);

impl RefSnapshot {
    /// Parse `git ls-remote --refs` output. Ignore advertised service refs;
    /// only heads and tags are portable migration candidates in this slice.
    pub fn parse(input: &str) -> Result<Self, String> {
        let mut refs = BTreeMap::new();
        let mut format_len = None;
        for (index, line) in input.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let error = || format!("invalid ref snapshot at line {}", index + 1);
            let mut fields = line.split_whitespace();
            let oid = fields.next().ok_or_else(error)?;
            let name = fields.next().ok_or_else(error)?;
            if fields.next().is_some()
                || !matches!(oid.len(), 40 | 64)
                || !oid.bytes().all(|b| b.is_ascii_hexdigit())
                || oid.bytes().all(|b| b == b'0')
            {
                return Err(error());
            }
            if let Some(len) = format_len {
                if len != oid.len() {
                    return Err("mixed object formats in snapshot".into());
                }
            }
            format_len = Some(oid.len());
            if !(name.starts_with("refs/heads/") || name.starts_with("refs/tags/")) {
                continue;
            }
            if !valid_ref_name(name) || refs.insert(name.into(), oid.to_ascii_lowercase()).is_some()
            {
                return Err(error());
            }
        }
        Ok(Self(refs))
    }
}

fn valid_ref_name(name: &str) -> bool {
    !name.contains("..")
        && !name.contains("@{")
        && !name.ends_with('.')
        && !name
            .bytes()
            .any(|b| b <= 32 || b == 127 || b"~^:?*[\\".contains(&b))
        && name
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefChange {
    CreateCandidate,
    Unchanged,
    /// Different hashes do not prove ancestry. A later object-graph check is required.
    NeedsAncestryCheck,
    /// Absence at source never authorizes a destination deletion.
    DestinationOnlyPreserve,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRef {
    pub name: String,
    pub source_oid: Option<String>,
    pub destination_oid: Option<String>,
    pub change: RefChange,
}

/// A deterministic preview, never an authorization to push. Live destination
/// refs must be checked again before execution to prevent stale-plan overwrites.
pub fn compare(source: &RefSnapshot, destination: &RefSnapshot) -> Result<Vec<PlannedRef>, String> {
    if let (Some(s), Some(d)) = (source.0.values().next(), destination.0.values().next()) {
        if s.len() != d.len() {
            return Err("source and destination use incompatible object formats".into());
        }
    }
    let names: BTreeSet<_> = source.0.keys().chain(destination.0.keys()).collect();
    Ok(names
        .into_iter()
        .map(|name| {
            let s = source.0.get(name);
            let d = destination.0.get(name);
            let change = match (s, d) {
                (Some(_), None) => RefChange::CreateCandidate,
                (None, Some(_)) => RefChange::DestinationOnlyPreserve,
                (Some(a), Some(b)) if a == b => RefChange::Unchanged,
                _ => RefChange::NeedsAncestryCheck,
            };
            PlannedRef {
                name: name.clone(),
                source_oid: s.cloned(),
                destination_oid: d.cloned(),
                change,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(n: char, name: &str) -> String {
        format!("{}\t{}\n", n.to_string().repeat(40), name)
    }

    #[test]
    fn previews_changes_without_authorizing_overwrite_or_delete() {
        let source = RefSnapshot::parse(
            &(row('a', "refs/heads/main")
                + &row('b', "refs/heads/new")
                + &row('c', "refs/tags/v1")),
        )
        .unwrap();
        let dest = RefSnapshot::parse(
            &(row('d', "refs/heads/main")
                + &row('e', "refs/heads/keep")
                + &row('c', "refs/tags/v1")),
        )
        .unwrap();
        let plan = compare(&source, &dest).unwrap();
        assert_eq!(
            plan.iter()
                .map(|p| (&*p.name, p.change))
                .collect::<Vec<_>>(),
            vec![
                ("refs/heads/keep", RefChange::DestinationOnlyPreserve),
                ("refs/heads/main", RefChange::NeedsAncestryCheck),
                ("refs/heads/new", RefChange::CreateCandidate),
                ("refs/tags/v1", RefChange::Unchanged),
            ]
        );
    }

    #[test]
    fn rejects_malformed_and_duplicate_refs() {
        for name in [
            "refs/heads/../escape",
            "refs/heads/.hidden",
            "refs/heads/x.lock",
            "refs/heads/x@{y",
            "refs/heads/",
            "refs/heads/x\\y",
        ] {
            assert!(RefSnapshot::parse(&row('a', name)).is_err(), "{name}");
        }
        assert!(
            RefSnapshot::parse(&(row('a', "refs/heads/main") + &row('b', "refs/heads/main")))
                .is_err()
        );
        assert!(RefSnapshot::parse(&row('0', "refs/heads/main")).is_err());
        assert!(RefSnapshot::parse("bad input").is_err());
    }

    #[test]
    fn ignores_host_internal_refs_and_handles_empty_repositories() {
        let source = RefSnapshot::parse(&row('a', "refs/pull/12/head")).unwrap();
        let empty = RefSnapshot::parse("").unwrap();
        assert!(compare(&source, &empty).unwrap().is_empty());
        let normal = RefSnapshot::parse(&row('a', "refs/heads/main")).unwrap();
        assert_eq!(
            compare(&normal, &empty).unwrap()[0].change,
            RefChange::CreateCandidate
        );
    }

    #[test]
    fn supports_sha256_but_rejects_mixed_formats() {
        let sha256 = RefSnapshot::parse(&format!("{} refs/heads/main", "a".repeat(64))).unwrap();
        assert_eq!(
            compare(&sha256, &sha256).unwrap()[0].change,
            RefChange::Unchanged
        );
        let sha1 = RefSnapshot::parse(&row('a', "refs/heads/main")).unwrap();
        assert!(compare(&sha1, &sha256).is_err());
        assert!(RefSnapshot::parse(&format!(
            "{}{} refs/heads/other",
            row('a', "refs/heads/main"),
            "b".repeat(64)
        ))
        .is_err());
    }
}
