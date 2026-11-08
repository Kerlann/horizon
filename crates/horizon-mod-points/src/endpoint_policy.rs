//! Registration-time resolution of declared internals into endpoint exposure plans.
use crate::exposure::{Grant, Layout, Plan};
use std::sync::Arc;

/// A point's virtual call layouts and special receiver layout.
#[derive(Clone, Debug)]
pub struct PointSpec {
    /// Head virtual call layout.
    pub head_call: usize,
    /// Tail virtual call layout, if supported.
    pub tail_call: Option<usize>,
    /// Receiver layout, which remains partial even through backreferences.
    pub receiver: Option<usize>,
}

/// One exact catalog entry which grants a receiver field on a particular layout.
#[derive(Clone, Debug)]
pub struct InternalField {
    /// Canonical internal path.
    pub path: String,
    /// Catalog member kind.
    pub kind: String,
    /// Exact catalog hash.
    pub hash: String,
    /// Receiver layout index.
    pub receiver: usize,
    /// Field name in the receiver layout.
    pub field: String,
}

/// Pure metadata emitted from the same graph as the native readers.
#[derive(Clone, Debug)]
pub struct SchemaCatalog {
    /// Ordered graph layouts.
    pub layouts: Vec<Layout>,
    /// Point metadata indexed by raw point index.
    pub points: Vec<PointSpec>,
    /// Exact catalog receiver-field entries.
    pub fields: Vec<InternalField>,
}

/// A manifest internal declaration, independent of package DTOs.
#[derive(Clone, Debug)]
pub struct InternalDeclaration {
    /// Canonical internal path.
    pub path: String,
    /// Optional catalog kind; absent requires an unambiguous path.
    pub kind: Option<String>,
    /// Pinned catalog hash.
    pub hash: String,
}

/// Manifest declarations to resolve before endpoint registration.
pub type ManifestInternals = [InternalDeclaration];

/// Why a manifest's internals declarations cannot become endpoint policies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InternalsRefusal {
    /// The game has no schema catalog to resolve declarations against.
    NoCatalog,
    /// No catalog entry has this path (and kind, when declared).
    UnknownPath(String),
    /// The path names several catalog kinds and the declaration gave none.
    AmbiguousKind(String),
    /// The pinned hash is not the catalog entry's hash.
    HashMismatch(String),
    /// The entry is not a field of a catalog receiver layout.
    NotAField(String),
    /// The field's type cannot cross, so it cannot be granted.
    OpaqueReceiverMember(String),
    /// No point this module listens to has this declaration's receiver.
    NoListener(String),
    /// The module listens at a point the catalog does not describe.
    UnknownPoint(usize),
    /// The catalog's own graph is inconsistent.
    Graph(String),
}
impl std::fmt::Display for InternalsRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCatalog => write!(
                f,
                "this game has no internals catalog to resolve declared internals"
            ),
            Self::UnknownPath(path) => write!(f, "unknown internal {path}"),
            Self::AmbiguousKind(path) => write!(f, "internal {path} needs a kind"),
            Self::HashMismatch(path) => {
                write!(f, "internal {path} does not match this game's hash")
            }
            Self::NotAField(path) => write!(f, "internal {path} is not a catalog field"),
            Self::OpaqueReceiverMember(path) => write!(f, "internal {path} cannot cross to a mod"),
            Self::NoListener(path) => write!(
                f,
                "internal {path} is declared but no listened point can read it"
            ),
            Self::UnknownPoint(point) => write!(f, "listener on unknown point {point}"),
            Self::Graph(why) => write!(f, "{why}"),
        }
    }
}
impl std::error::Error for InternalsRefusal {}

/// Module-owned precomputed policies. Dropping this releases its plan owners.
pub struct EndpointPolicy {
    plans: std::collections::HashMap<usize, Arc<Plan>>,
}
impl EndpointPolicy {
    /// Validate every declaration and build all point policies atomically. `listened` holds
    /// the head call layouts of the points the module listens to.
    pub fn build(
        catalog: &SchemaCatalog,
        internals: &ManifestInternals,
        listened: &[usize],
    ) -> Result<Self, InternalsRefusal> {
        let mut points = Vec::new();
        for &head in listened {
            points.push(
                catalog
                    .points
                    .iter()
                    .find(|point| point.head_call == head)
                    .ok_or(InternalsRefusal::UnknownPoint(head))?,
            );
        }
        let mut grants = Vec::new();
        for declaration in internals {
            let path = &declaration.path;
            let named: Vec<_> = catalog
                .fields
                .iter()
                .filter(|entry| &entry.path == path)
                .collect();
            let matches: Vec<_> = named
                .iter()
                .filter(|entry| {
                    declaration
                        .kind
                        .as_ref()
                        .is_none_or(|kind| kind == &entry.kind)
                })
                .collect();
            let entry = match matches[..] {
                [] if named.is_empty() => return Err(InternalsRefusal::UnknownPath(path.clone())),
                // The path exists, but not as the declared kind: not a catalog field.
                [] => return Err(InternalsRefusal::NotAField(path.clone())),
                [entry] => *entry,
                _ => return Err(InternalsRefusal::AmbiguousKind(path.clone())),
            };
            if entry.hash != declaration.hash {
                return Err(InternalsRefusal::HashMismatch(path.clone()));
            }
            let layout = catalog
                .layouts
                .get(entry.receiver)
                .ok_or_else(|| InternalsRefusal::NotAField(path.clone()))?;
            let field = match layout
                .fields
                .iter()
                .filter(|field| field.name == entry.field)
                .collect::<Vec<_>>()[..]
            {
                [field] => field,
                _ => return Err(InternalsRefusal::NotAField(path.clone())),
            };
            if catalog
                .layouts
                .get(field.target)
                .is_some_and(|target| target.opaque.is_some())
            {
                return Err(InternalsRefusal::OpaqueReceiverMember(path.clone()));
            }
            if !points
                .iter()
                .any(|point| point.receiver == Some(entry.receiver))
            {
                return Err(InternalsRefusal::NoListener(path.clone()));
            }
            grants.push((
                entry.receiver,
                Grant {
                    case: field.case,
                    ordinal: field.ordinal,
                },
            ));
        }
        // Only listened points get a plan: a plan is sized by the whole graph.
        let mut plans = std::collections::HashMap::new();
        for point in points {
            let selected: Vec<_> = grants
                .iter()
                .filter_map(|(receiver, grant)| {
                    (Some(*receiver) == point.receiver).then_some(*grant)
                })
                .collect();
            let mut roots = vec![point.head_call];
            roots.extend(point.tail_call);
            plans.insert(
                point.head_call,
                Arc::new(
                    Plan::build(&catalog.layouts, point.receiver, &selected, &roots)
                        .map_err(InternalsRefusal::Graph)?,
                ),
            );
        }
        Ok(Self { plans })
    }
    /// Return an owned plan for the point's head call layout, as named in its staged plan.
    pub fn plan(&self, head_call: usize) -> Option<Arc<Plan>> {
        self.plans.get(&head_call).cloned()
    }
}
