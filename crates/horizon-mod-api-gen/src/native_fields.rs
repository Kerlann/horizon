//! Finite native-place projections for raw point snapshots and sparse writeback.
//!
//! This emits code only; the caller owns the borrow scope and must use the same base node id
//! when constructing both expressions. No generated value contains a native address.
use crate::crossing::{self, Rule, Scalar, Shape};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Type;

#[cfg(test)]
#[path = "native_fields_tests.rs"]
mod tests;

/// A projection's schema identity and generated expressions.
pub struct Projection {
    /// Number of consecutive snapshot nodes, including the root.
    pub nodes: u32,
    /// Canonical layout identity. Changes to reachable field layouts change this string.
    pub identity: String,
    /// Expression returning `Result<(), WireError>`, appending to the supplied builder.
    pub snapshot: TokenStream,
    /// Expression constructing a `PatchFields` over the original mutable places.
    pub patch: TokenStream,
}

struct Node {
    field: u32,
    identity: String,
    kind: Kind,
}
enum Kind {
    Leaf,
    Ref(bool, Box<Node>),
    Fields(Vec<(syn::Member, Node)>),
    Array(Vec<Node>),
}

/// Resolve `ty` in its declaring module and emit projections of `place`.
///
/// `place` must be a stable native place, evaluated once per expression. `builder` is a mutable
/// Builder place; `base` is its node count before this projection (and the patch's id offset).
/// `root_parent` and `root_field` locate this projection beneath the containing call node.
/// `writable` is the producer's authorization, not an inference from the native type. Reference
/// targets are walked without exposing a retargetable reference; shared references stop writes.
/// The caller must not evaluate either expression after consuming a non-Copy native argument.
pub fn generate(
    rule: &Rule,
    module: &str,
    owner: Option<&str>,
    ty: &Type,
    place: TokenStream,
    builder: TokenStream,
    base: u32,
    root_parent: Option<u32>,
    root_field: u32,
    writable: bool,
) -> Result<Projection, String> {
    fn build(
        rule: &Rule,
        shape: &Shape,
        field: u32,
        open: &mut Vec<String>,
        count: &mut u32,
    ) -> Result<Node, String> {
        *count = count
            .checked_add(1)
            .ok_or("unimplemented: projection too large")?;
        if *count > 65_536 || open.len() > 128 {
            return Err("unimplemented: projection exceeds finite generation limits".into());
        }
        let (identity, kind) = match shape {
            Shape::Scalar(s)
                if !matches!(s, Scalar::String | Scalar::Str | Scalar::Signed("isize")) =>
            {
                let name = match s {
                    Scalar::Bool => "bool",
                    Scalar::Char => "char",
                    Scalar::Signed(n)
                    | Scalar::Unsigned(n)
                    | Scalar::Float(n)
                    | Scalar::Wide(n) => n,
                    Scalar::String => "String",
                    Scalar::Str => unreachable!(),
                };
                (name.to_string(), Kind::Leaf)
            }
            Shape::Unit => ("()".into(), Kind::Leaf),
            Shape::Ref { mutable, to } => {
                // References do not introduce an extra row: their target has the same node id.
                *count -= 1;
                let inner = build(rule, to, field, open, count)?;
                return Ok(Node {
                    field,
                    identity: inner.identity.clone(),
                    kind: Kind::Ref(*mutable, Box::new(inner)),
                });
            }
            Shape::Tuple(parts) => {
                let mut fields = Vec::new();
                for (i, part) in parts.iter().enumerate() {
                    fields.push((
                        syn::Member::Unnamed(i.into()),
                        build(rule, part, i as u32, open, count)?,
                    ));
                }
                let identity = format!(
                    "tuple({})",
                    fields
                        .iter()
                        .map(|(_, n)| n.identity.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                );
                (identity, Kind::Fields(fields))
            }
            Shape::Array(part, len) => {
                if *len > 65_536 {
                    return Err("unimplemented: array too large".into());
                }
                let mut children = Vec::new();
                for i in 0..*len {
                    children.push(build(rule, part, i as u32, open, count)?);
                }
                let element_identity = if let Some(child) = children.first() {
                    child.identity.clone()
                } else {
                    let mut ignored = 0;
                    build(rule, part, 0, open, &mut ignored)?.identity
                };
                let identity = format!("array({len};{element_identity})");
                (identity, Kind::Array(children))
            }
            Shape::Named(path, _) => {
                let named = rule.named(path).ok_or("unimplemented: missing layout")?;
                if named.guarded
                    || named.opaque_reason.is_some()
                    || crossing::foreign(path) == Some(crossing::Foreign::Bits)
                    || !matches!(named.item, syn::Item::Struct(_))
                    || open.contains(path)
                {
                    return Err(format!(
                        "unimplemented: inaccessible, recursive or non-struct layout {path}"
                    ));
                }
                // Foreign fields are not widened by the staged game.
                if named.foreign
                    && let syn::Item::Struct(item) = &named.item
                    && item
                        .fields
                        .iter()
                        .any(|f| !matches!(f.vis, syn::Visibility::Public(_)))
                {
                    return Err(format!(
                        "unimplemented: inaccessible foreign fields of {path}"
                    ));
                }
                open.push(path.clone());
                let mut fields = Vec::new();
                for f in rule.fields(path) {
                    let child_shape = rule.shape(&named.module, Some(path), f.ty);
                    let index = fields.len() as u32;
                    fields.push((f.member, build(rule, &child_shape, index, open, count)?));
                }
                open.pop();
                let layout = fields
                    .iter()
                    .map(|(m, n)| format!("{}:{}", quote!(#m), n.identity))
                    .collect::<Vec<_>>()
                    .join(",");
                (format!("struct({path};{layout})"), Kind::Fields(fields))
            }
            _ => {
                return Err(format!(
                    "unimplemented: finite native projection for {shape:?}"
                ));
            }
        };
        Ok(Node {
            field,
            identity,
            kind,
        })
    }
    let shape = rule.shape(module, owner, ty);
    let mut nodes = 0;
    if root_parent.is_some_and(|parent| parent >= base) {
        return Err("unimplemented: root parent must precede projection".into());
    }
    let tree = build(rule, &shape, root_field, &mut Vec::new(), &mut nodes)?;
    base.checked_add(nodes)
        .ok_or("unimplemented: node id overflow")?;
    let mut next = base;
    let mut snapshots = Vec::new();
    let mut setup = Vec::new();
    let mut cells = Vec::new();
    fn emit(
        node: &Node,
        place: TokenStream,
        parent: Option<u32>,
        writable: bool,
        next: &mut u32,
        snapshots: &mut Vec<TokenStream>,
        setup: &mut Vec<TokenStream>,
        cells: &mut Vec<TokenStream>,
        builder: &TokenStream,
    ) {
        if let Kind::Ref(mutable, inner) = &node.kind {
            emit(
                inner,
                quote!(*(#place)),
                parent,
                writable && *mutable,
                next,
                snapshots,
                setup,
                cells,
                builder,
            );
            return;
        }
        let id = *next;
        *next += 1;
        let field = node.field;
        let key = &node.identity;
        let parent = match parent {
            Some(p) => quote!(Some(#p)),
            None => quote!(None),
        };
        match &node.kind {
            Kind::Leaf => {
                snapshots.push(quote! { (#builder).value(#parent, #field, const { mod_points::snapshot::key(#key) }, &(#place))?; });
                if writable {
                    cells.push(quote! { mod_api::wire::PatchCell::new(#id, &mut (#place)) });
                }
            }
            Kind::Fields(fields) => {
                snapshots.push(
                    quote! { (#builder).node(#parent, #field, const { mod_points::snapshot::key(#key) })?; },
                );
                for (member, child) in fields {
                    emit(
                        child,
                        quote!((#place).#member),
                        Some(id),
                        writable,
                        next,
                        snapshots,
                        setup,
                        cells,
                        builder,
                    );
                }
            }
            Kind::Array(children) => {
                let length = children.len() as u64;
                snapshots.push(
                    quote! { (#builder).value(#parent, #field, const { mod_points::snapshot::key(#key) }, &#length)?; },
                );
                // Snapshot and patch places differ for arrays: destructure once to prove disjointness.
                let names: Vec<_> = (0..children.len())
                    .map(|i| format_ident!("__horizon_array_{}_{}", id, i))
                    .collect();
                if writable {
                    setup.push(quote! { let [#(#names),*] = &mut (#place); });
                }
                for (i, child) in children.iter().enumerate() {
                    let index = syn::Index::from(i);
                    // The snapshot reads through indexing; patch paths are rewritten below.
                    let mut child_setup = Vec::new();
                    let mut child_cells = Vec::new();
                    let mut ignored = Vec::new();
                    let start = *next;
                    emit(
                        child,
                        quote!((#place)[#index]),
                        Some(id),
                        false,
                        next,
                        snapshots,
                        &mut ignored,
                        &mut Vec::new(),
                        builder,
                    );
                    if writable {
                        let mut patch_next = start;
                        let name = &names[i];
                        emit(
                            child,
                            quote!(*#name),
                            Some(id),
                            true,
                            &mut patch_next,
                            &mut Vec::new(),
                            &mut child_setup,
                            &mut child_cells,
                            builder,
                        );
                        setup.extend(child_setup);
                        cells.extend(child_cells);
                    }
                }
            }
            Kind::Ref(..) => unreachable!(),
        }
    }
    emit(
        &tree,
        quote!(*__horizon_root),
        root_parent,
        writable,
        &mut next,
        &mut snapshots,
        &mut setup,
        &mut cells,
        &builder,
    );
    let fields = cells
        .into_iter()
        .rev()
        .fold(quote!(()), |tail, cell| quote!((#cell, #tail)));
    let end = base + nodes;
    Ok(Projection {
        nodes,
        identity: tree.identity,
        snapshot: quote!((|| { let __horizon_root = &(#place); #(#snapshots)* Ok::<(), mod_api::wire::WireError>(()) })()),
        patch: if writable {
            quote!({ let __horizon_root = &mut (#place); #(#setup)* mod_api::wire::PatchFields::new(#fields, #end) })
        } else {
            quote!(mod_api::wire::PatchFields::new((), #end))
        },
    })
}
