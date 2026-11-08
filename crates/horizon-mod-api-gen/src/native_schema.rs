//! Canonical identities for native row projections.
//!
//! Identities include the reachable layout and the producer's exposure policy. Recursive named
//! types refer to their nearest enclosing definition by path instead of expanding indefinitely.
use crate::crossing::{self, Collection, Rule, Scalar, Shape};
use quote::quote;

/// Selects declaration-order fields of each named type. The policy is part of the identity.
/// Case is `None` for a struct and the declaration-order case index for an enum.
pub trait Exposure {
    /// Stable policy name; two policies that expose different fields must use different names.
    fn name(&self) -> &str;
    /// Whether a field is visible in this projection.
    fn field(&self, path: &str, case: Option<u32>, field: u32) -> bool;
}

/// Expose every accessible field.
pub struct All;
impl Exposure for All {
    fn name(&self) -> &str {
        "all"
    }
    fn field(&self, _: &str, _: Option<u32>, _: u32) -> bool {
        true
    }
}

/// Canonical identity for `shape`, including the selected reachable native layout.
/// Opaque or inaccessible layouts are refused rather than assigned a misleading identity.
pub fn identity(rule: &Rule, shape: &Shape, exposure: &dyn Exposure) -> Result<String, String> {
    fn walk(
        rule: &Rule,
        shape: &Shape,
        exposure: &dyn Exposure,
        open: &mut Vec<String>,
    ) -> Result<String, String> {
        let child = |s: &Shape, open: &mut Vec<String>| walk(rule, s, exposure, open);
        Ok(match shape {
            Shape::Scalar(s) => match s {
                Scalar::Bool => "bool".into(),
                Scalar::Char => "char".into(),
                Scalar::Signed(n) | Scalar::Unsigned(n) | Scalar::Float(n) | Scalar::Wide(n) => {
                    (*n).into()
                }
                Scalar::Str | Scalar::String => "utf16".into(),
            },
            Shape::Unit => "unit".into(),
            Shape::Ref { to, .. } | Shape::Wrapped(_, to) => child(to, open)?,
            Shape::Tuple(parts) => format!(
                "tuple({})",
                parts
                    .iter()
                    .map(|p| child(p, open))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(",")
            ),
            Shape::Array(part, n) => format!("array({n};{})", child(part, open)?),
            Shape::Slice(part) => format!("sequence({})", child(part, open)?),
            Shape::Option(part) => format!("option({})", child(part, open)?),
            Shape::Collection(kind, args) => {
                let count = match kind {
                    Collection::HashMap | Collection::BTreeMap => 2,
                    _ => 1,
                };
                if args.len() < count {
                    return Err("unimplemented: malformed collection".into());
                }
                let name = match kind {
                    Collection::Vec | Collection::VecDeque => "sequence",
                    Collection::HashSet | Collection::BTreeSet => "set",
                    _ => "map",
                };
                format!(
                    "{name}({})",
                    args[..count]
                        .iter()
                        .map(|p| child(p, open))
                        .collect::<Result<Vec<_>, _>>()?
                        .join(",")
                )
            }
            Shape::Named(path, _) => {
                let named = rule
                    .named(path)
                    .ok_or_else(|| format!("unimplemented: missing {path}"))?;
                if named.guarded
                    || named.opaque_reason.is_some()
                    || crossing::foreign(path) == Some(crossing::Foreign::Bits)
                {
                    return Err(format!("unimplemented: inaccessible {path}"));
                }
                if open.contains(path) {
                    return Ok(format!("ref({path})"));
                }
                open.push(path.clone());
                if named.foreign {
                    let inaccessible = match &named.item {
                        syn::Item::Struct(item) => item
                            .fields
                            .iter()
                            .any(|f| !matches!(f.vis, syn::Visibility::Public(_))),
                        syn::Item::Enum(_) => false,
                        _ => true,
                    };
                    if inaccessible {
                        return Err(format!(
                            "unimplemented: inaccessible foreign fields of {path}"
                        ));
                    }
                }
                let fields = rule.fields(path);
                let mut out = Vec::new();
                for (i, f) in fields.iter().enumerate() {
                    if !exposure.field(path, f.case.map(|c| c.0), i as u32) {
                        continue;
                    }
                    let shape = rule.shape(&named.module, Some(path), f.ty);
                    let member = &f.member;
                    out.push(format!(
                        "{}:{}:{}",
                        f.case.map_or_else(|| "-".into(), |c| c.0.to_string()),
                        quote!(#member),
                        child(&shape, open)?
                    ));
                }
                let cases = if let syn::Item::Enum(e) = &named.item {
                    e.variants
                        .iter()
                        .map(|v| v.ident.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                } else {
                    String::new()
                };
                open.pop();
                format!(
                    "named({path};{};{cases};{})",
                    exposure.name(),
                    out.join(",")
                )
            }
            Shape::Opaque(_) => return Err(format!("unimplemented: opaque shape {shape:?}")),
        })
    }
    Ok(format!(
        "exposure({};{})",
        exposure.name(),
        walk(rule, shape, exposure, &mut Vec::new())?
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crossing::tests::internal;

    struct First;
    impl Exposure for First {
        fn name(&self) -> &str {
            "first"
        }
        fn field(&self, _: &str, _: Option<u32>, index: u32) -> bool {
            index == 0
        }
    }

    #[test]
    fn fingerprints_layout_and_exposure_and_terminates_recursive_graphs() {
        let make = |s| Rule::new(&[internal("horizon_client::Node", "struct", s)]);
        let a = make("struct Node { a: u8, next: Option<Box<Node>> }");
        let b = make("struct Node { a: u16, next: Option<Box<Node>> }");
        let shape = a.shape("horizon_client", None, &syn::parse_str("Node").unwrap());
        let full = identity(&a, &shape, &All).unwrap();
        assert!(full.contains("ref(horizon_client::Node)"));
        assert_ne!(full, identity(&b, &shape, &All).unwrap());
        let first = identity(&a, &shape, &First).unwrap();
        assert_ne!(full, first);
        assert!(!first.contains("next"));
    }

    #[test]
    fn guarded_layout_is_refused() {
        let rule = Rule::new(&[internal(
            "horizon_client::Node",
            "struct",
            "struct Node { #[cfg(x)] a: u8 }",
        )]);
        let shape = rule.shape("horizon_client", None, &syn::parse_str("Node").unwrap());
        assert!(identity(&rule, &shape, &All).is_err());
    }
}
