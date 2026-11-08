//! Which crate's helper module can emit which layout of the shared catalog graph.
//!
//! Every crate that owns points gets its own `__horizon_points` module at its library root. A
//! layout is spelled by the path the catalog knows it under, which names it from the crate that
//! declares it (`extern crate self as <crate>` makes that true in the crate itself) or from a
//! crate that is a direct dependency of the emitting crate. A helper for a layout whose
//! reachable layouts cannot all be named from the emitting crate is never emitted, and a point
//! reaching one is refused rather than a visibility widened.
use crate::{crossing::Rule, native_policy::Graph};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// The crate a catalog path or module belongs to.
pub fn owner(path: &str) -> &str {
    path.split("::").next().unwrap_or_default()
}

/// What a crate's source can name: itself and its direct dependencies.
pub struct Names {
    krate: String,
    dependencies: BTreeSet<String>,
}

impl Names {
    /// The names of `krate`, from the `[dependencies]` of its manifest.
    pub fn read(manifest: &toml::Value, krate: &str) -> Self {
        Self {
            krate: krate.to_owned(),
            dependencies: manifest
                .get("dependencies")
                .and_then(toml::Value::as_table)
                .into_iter()
                .flatten()
                .filter(|(_, dep)| dep.get("optional").and_then(toml::Value::as_bool) != Some(true))
                .map(|(name, _)| name.replace('-', "_"))
                .collect(),
        }
    }

    /// Whether the crate's source can write `path`.
    pub fn names(&self, path: &str) -> bool {
        let head = owner(path);
        head == self.krate
            || matches!(head, "std" | "core" | "alloc")
            || self.dependencies.contains(head)
    }

    /// For each layout, a non-opaque named layout reachable from it (itself included) that this
    /// crate cannot name. An opaque layout stops the walk: its members are never emitted.
    pub fn blocked(&self, rule: &Rule, graph: &Graph) -> Vec<Option<usize>> {
        let layouts = &graph.layouts;
        let mut parents = vec![Vec::new(); layouts.len()];
        for (id, layout) in layouts.iter().enumerate() {
            for target in layout
                .fields
                .iter()
                .map(|field| field.target)
                .chain(layout.children.iter().copied())
            {
                parents[target].push(id);
            }
        }
        let mut blocked = vec![None; layouts.len()];
        let mut queue = VecDeque::new();
        for (id, layout) in layouts.iter().enumerate() {
            if layout.opaque.is_none()
                && rule.named(&layout.name).is_some()
                && !self.names(&layout.name)
            {
                blocked[id] = Some(id);
                queue.push_back(id);
            }
        }
        while let Some(id) = queue.pop_front() {
            for &parent in &parents[id] {
                if blocked[parent].is_none() && layouts[parent].opaque.is_none() {
                    blocked[parent] = blocked[id];
                    queue.push_back(parent);
                }
            }
        }
        blocked
    }
}

/// The layouts reachable from `roots`, not walking into opaque ones.
pub fn reachable(graph: &Graph, roots: impl IntoIterator<Item = usize>) -> Vec<bool> {
    let mut seen = vec![false; graph.layouts.len()];
    let mut stack: Vec<usize> = roots.into_iter().collect();
    while let Some(id) = stack.pop() {
        if std::mem::replace(&mut seen[id], true) {
            continue;
        }
        let layout = &graph.layouts[id];
        if layout.opaque.is_none() {
            stack.extend(layout.fields.iter().map(|field| field.target));
            stack.extend(layout.children.iter().copied());
        }
    }
    seen
}

/// Which layouts a crate's helper module emits: those its points reach and its source can name
/// in full. `calls` are the registered call layouts of the crate's admitted points.
pub fn emitted(
    rule: &Rule,
    graph: &Graph,
    blocked: &[Option<usize>],
    calls: &[usize],
) -> Vec<bool> {
    let mut emit = reachable(graph, calls.iter().copied());
    for (id, layout) in graph.layouts.iter().enumerate() {
        emit[id] &=
            blocked[id].is_none() && layout.opaque.is_none() && rule.named(&layout.name).is_some();
    }
    emit
}

/// Why a point whose call layouts are `calls` cannot be read from `krate`, if it cannot.
pub fn refusal(
    graph: &Graph,
    blocked: &[Option<usize>],
    krate: &str,
    calls: &[usize],
) -> Option<String> {
    calls.iter().find_map(|&call| blocked[call]).map(|layout| {
        format!(
            "layout {} is not nameable from crate {krate}",
            graph.layouts[layout].name
        )
    })
}

/// The names of each crate in `crates`, from its manifest under `root`.
pub fn read(
    root: &std::path::Path,
    crates: &BTreeSet<String>,
) -> Result<BTreeMap<String, Names>, String> {
    crates
        .iter()
        .map(|krate| {
            let (package, _) = crate::points::crate_root(root, krate)?;
            let manifest = package.join("Cargo.toml");
            let text = std::fs::read_to_string(&manifest)
                .map_err(|e| format!("{}: {e}", manifest.display()))?;
            let manifest: toml::Value = toml::from_str(&text).map_err(|e| e.to_string())?;
            Ok((krate.clone(), Names::read(&manifest, krate)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{crossing::Shape, internals::Internal};

    fn internal(path: &str, signature: &str) -> Internal {
        Internal {
            path: path.into(),
            kind: "struct",
            cfg: String::new(),
            signature: signature.into(),
            hash: String::new(),
            shape_only: true,
            opaque_reason: None,
            refs: Default::default(),
        }
    }

    fn names(krate: &str, dependencies: &[&str]) -> Names {
        let manifest: toml::Value = toml::from_str(&format!(
            "[dependencies]\n{}",
            dependencies
                .iter()
                .map(|name| format!("{name} = {{ path = \"../{name}\" }}\n"))
                .collect::<String>()
        ))
        .unwrap();
        Names::read(&manifest, krate)
    }

    #[test]
    fn a_crate_names_itself_its_dependencies_and_std_only() {
        let names = names("lower", &["horizon-below", "other"]);
        assert!(names.names("lower::a::A"));
        assert!(names.names("horizon_below::B"));
        assert!(names.names("std::time::Duration"));
        assert!(!names.names("horizon_client::C"));
        assert!(!names.names("transitive::D"));
    }

    #[test]
    fn a_layout_reaching_an_unnameable_one_is_blocked_unless_opaque_stops_it() {
        let rule = Rule::new(&[
            internal(
                "lower::Own",
                "struct Own { pub a: horizon_below::Below, pub b: transitive::Far }",
            ),
            internal("lower::Near", "struct Near { pub a: horizon_below::Below }"),
            internal("horizon_below::Below", "struct Below { pub x: u8 }"),
            internal("transitive::Far", "struct Far { pub x: u8 }"),
        ]);
        let mut graph = Graph::default();
        let own = graph.add(&rule, &Shape::Named("lower::Own".into(), 0));
        let near = graph.add(&rule, &Shape::Named("lower::Near".into(), 0));
        let far = graph
            .id(&Shape::Named("transitive::Far".into(), 0))
            .unwrap();
        let names = names("lower", &["horizon-below"]);
        let blocked = names.blocked(&rule, &graph);
        assert_eq!(blocked[own], Some(far));
        assert_eq!(blocked[near], None);
        assert_eq!(blocked[far], Some(far));
        assert_eq!(
            refusal(&graph, &blocked, "lower", &[own]).unwrap(),
            "layout transitive::Far is not nameable from crate lower"
        );
        assert_eq!(refusal(&graph, &blocked, "lower", &[near]), None);
        let mut opaque = graph;
        opaque.layouts[far].opaque = Some("sealed".into());
        let blocked = names.blocked(&rule, &opaque);
        assert_eq!(blocked[own], None);
        assert_eq!(blocked[far], None);
    }

    #[test]
    fn a_crate_emits_only_what_its_points_reach_and_can_name() {
        let rule = Rule::new(&[
            internal("lower::Used", "struct Used { pub a: horizon_below::Below }"),
            internal("lower::Unused", "struct Unused { pub x: u8 }"),
            internal("horizon_below::Below", "struct Below { pub x: u8 }"),
            internal("transitive::Far", "struct Far { pub x: u8 }"),
            internal("lower::Bad", "struct Bad { pub a: transitive::Far }"),
        ]);
        let mut graph = Graph::default();
        let used = graph.add(&rule, &Shape::Named("lower::Used".into(), 0));
        let unused = graph.add(&rule, &Shape::Named("lower::Unused".into(), 0));
        let bad = graph.add(&rule, &Shape::Named("lower::Bad".into(), 0));
        let below = graph
            .id(&Shape::Named("horizon_below::Below".into(), 0))
            .unwrap();
        let blocked = names("lower", &["horizon-below"]).blocked(&rule, &graph);
        let emit = emitted(&rule, &graph, &blocked, &[used, bad]);
        assert!(emit[used] && emit[below]);
        assert!(!emit[unused], "no point reaches it");
        assert!(!emit[bad], "it needs a layout the crate cannot name");
    }
}
