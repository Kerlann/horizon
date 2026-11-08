//! Source-local exposure plans, computed once at endpoint registration.
//!
//! Descriptors contain schema metadata only, never native pointers or engine values. Type IDs
//! are descriptor indices local to one source. Readers retain the resulting plan and use indexed
//! lookups; neither field reads nor schema-key reads walk the graph or hash strings.
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// One declared member, using its original declaration ordinal (not a packed visible index).
#[derive(Clone, Debug)]
pub struct Field {
    /// Enum case, or `None` for a struct.
    pub case: Option<u32>,
    /// Case-local declaration ordinal.
    pub ordinal: u32,
    /// Canonical member spelling.
    pub name: String,
    /// Target descriptor index. References are transparent in the schema graph.
    pub target: usize,
}

/// Metadata emitted once by a source reader generator.
#[derive(Clone, Debug)]
pub struct Layout {
    /// Canonical type identity, independent of descriptor ordering.
    pub name: String,
    /// Full local layout signature, including scalar widths, cases and container kind.
    pub signature: String,
    /// Declared fields (empty for leaves).
    pub fields: Vec<Field>,
    /// Container element / tuple component edges, always traversed when reachable.
    pub children: Vec<usize>,
    /// Explicit reason this layout cannot cross.
    pub opaque: Option<String>,
}

/// An explicitly granted receiver field.
#[derive(Clone, Copy, Debug)]
pub struct Grant {
    /// Enum case, or `None` for a struct.
    pub case: Option<u32>,
    /// Case-local declaration ordinal.
    pub ordinal: u32,
}

/// Precomputed source-local field admission and selected-layout keys.
#[derive(Debug)]
pub struct Plan {
    fields: Vec<HashSet<(Option<u32>, u32)>>,
    keys: Vec<Option<u64>>,
    complete: Vec<bool>,
}

impl Plan {
    /// Register one endpoint's policy. The receiver remains partial even when an argument graph
    /// refers back to its type. Ordinary argument roots and selected receiver members otherwise
    /// reach accessible layouts, with opaque argument layouts retained as nonconstructible leaves.
    /// Unknown grants and explicitly granted opaque receiver members fail closed.
    pub fn build(
        layouts: &[Layout],
        receiver: Option<usize>,
        grants: &[Grant],
        arguments: &[usize],
    ) -> Result<Self, String> {
        let mut names = BTreeSet::new();
        for layout in layouts {
            if !names.insert(&layout.name) {
                return Err(format!("duplicate layout {}", layout.name));
            }
            let mut members = BTreeSet::new();
            for field in &layout.fields {
                if field.target >= layouts.len() || !members.insert((field.case, field.ordinal)) {
                    return Err(format!("invalid member in {}", layout.name));
                }
            }
            if layout.children.iter().any(|&id| id >= layouts.len()) {
                return Err(format!("invalid child in {}", layout.name));
            }
        }
        if receiver.is_some_and(|id| id >= layouts.len())
            || arguments.iter().any(|&id| id >= layouts.len())
        {
            return Err("unknown root".into());
        }
        if receiver.is_none() && !grants.is_empty() {
            return Err("field grant without receiver".into());
        }
        let selected: BTreeSet<_> = grants.iter().map(|g| (g.case, g.ordinal)).collect();
        if let Some(id) = receiver
            && selected
                .iter()
                .any(|g| !layouts[id].fields.iter().any(|f| (f.case, f.ordinal) == *g))
        {
            return Err("unknown receiver member".into());
        }
        let mut fields = vec![HashSet::new(); layouts.len()];
        let mut reachable = BTreeSet::new();
        let mut pending = arguments.to_vec();
        pending.extend(receiver);
        while let Some(id) = pending.pop() {
            if !reachable.insert(id) {
                continue;
            }
            let layout = &layouts[id];
            if layout.opaque.is_some() {
                if Some(id) == receiver
                    || receiver.is_some_and(|r| {
                        layouts[r]
                            .fields
                            .iter()
                            .any(|f| selected.contains(&(f.case, f.ordinal)) && f.target == id)
                    })
                {
                    return Err(format!("{}: opaque receiver member", layout.name));
                }
                continue;
            }
            pending.extend(&layout.children);
            for field in &layout.fields {
                let member = (field.case, field.ordinal);
                if Some(id) != receiver || selected.contains(&member) {
                    fields[id].insert(member);
                    pending.push(field.target);
                }
            }
        }
        // Each key fingerprints its entire permitted reachable graph. Sorting by canonical name
        // makes registration and descriptor order irrelevant, and finite type refs handle cycles.
        let mut keys = vec![None; layouts.len()];
        for &root in &reachable {
            let mut graph = BTreeMap::new();
            let mut pending = vec![root];
            while let Some(id) = pending.pop() {
                if graph.insert(&layouts[id].name, id).is_some() {
                    continue;
                }
                if layouts[id].opaque.is_some() {
                    continue;
                }
                pending.extend(&layouts[id].children);
                pending.extend(
                    layouts[id]
                        .fields
                        .iter()
                        .filter(|f| fields[id].contains(&(f.case, f.ordinal)))
                        .map(|f| f.target),
                );
            }
            let mut canonical = format!("projection-v1:{}:", layouts[root].name.len());
            canonical.push_str(&layouts[root].name);
            for (_, id) in graph {
                let l = &layouts[id];
                for part in [&l.name, &l.signature] {
                    canonical.push_str(&format!("{}:{part}", part.len()));
                }
                for &child in &l.children {
                    let n = &layouts[child].name;
                    canonical.push_str(&format!("c{}:{n}", n.len()));
                }
                let mut members: Vec<_> = l
                    .fields
                    .iter()
                    .filter(|f| fields[id].contains(&(f.case, f.ordinal)))
                    .collect();
                members.sort_by_key(|f| (f.case, f.ordinal));
                for f in members {
                    let n = &layouts[f.target].name;
                    canonical.push_str(&format!(
                        "f{:?}:{}:{}:{}:{}:{n}",
                        f.case,
                        f.ordinal,
                        f.name.len(),
                        f.name,
                        n.len()
                    ));
                }
            }
            let mut hash = 0xcbf29ce484222325u64;
            for byte in canonical.bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
            keys[root] = Some(hash);
        }
        let mut complete = vec![false; layouts.len()];
        for &root in &reachable {
            let mut seen = BTreeSet::new();
            let mut pending = vec![root];
            let mut allowed = true;
            while let Some(id) = pending.pop() {
                if !seen.insert(id) {
                    continue;
                }
                let layout = &layouts[id];
                if layout.opaque.is_some()
                    || layout
                        .fields
                        .iter()
                        .any(|f| !fields[id].contains(&(f.case, f.ordinal)))
                {
                    allowed = false;
                    break;
                }
                pending.extend(&layout.children);
                pending.extend(layout.fields.iter().map(|f| f.target));
            }
            complete[root] = allowed;
        }
        Ok(Self {
            fields,
            keys,
            complete,
        })
    }

    /// Whether the entire reachable layout is exposed and constructible.
    pub fn complete(&self, id: usize) -> bool {
        self.complete.get(id).copied().unwrap_or(false)
    }

    /// Selected-layout schema key, or `None` for a type not reachable from this endpoint.
    pub fn key(&self, id: usize) -> Option<u64> {
        self.keys.get(id).copied().flatten()
    }

    /// Whether a declaration-order member is permitted. Unknown types and members refuse.
    pub fn field(&self, id: usize, case: Option<u32>, ordinal: u32) -> bool {
        self.fields
            .get(id)
            .is_some_and(|f| f.contains(&(case, ordinal)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_tracks_cycles_partial_members_and_opaque_arguments() {
        let mut layouts = vec![Layout {
            name: "Node".into(),
            signature: "Node".into(),
            fields: vec![Field {
                case: None,
                ordinal: 0,
                name: "next".into(),
                target: 0,
            }],
            children: vec![],
            opaque: None,
        }];
        let full = Plan::build(&layouts, None, &[], &[0]).unwrap();
        assert!(full.complete(0));
        let partial = Plan::build(&layouts, Some(0), &[], &[]).unwrap();
        assert!(!partial.complete(0));
        assert!(!partial.complete(99));
        layouts.push(Layout {
            name: "Opaque".into(),
            signature: "opaque".into(),
            fields: vec![],
            children: vec![],
            opaque: Some("opaque".into()),
        });
        layouts[0].fields[0].target = 1;
        let opaque = Plan::build(&layouts, None, &[], &[0]).unwrap();
        assert!(opaque.key(1).is_some());
        assert!(!opaque.complete(0));
        assert!(!opaque.complete(1));
        assert!(
            Plan::build(
                &layouts,
                Some(0),
                &[Grant {
                    case: None,
                    ordinal: 0
                }],
                &[]
            )
            .is_err()
        );
    }
}
