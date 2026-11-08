//! Rule-to-runtime schema graph emission. This graph is shared by guest and native registration.
use crate::{
    crossing::{Collection, Rule, Shape},
    exposure::{Field, Layout},
};
use quote::quote;
#[cfg(test)]
#[path = "native_policy_tests.rs"]
mod tests;
use std::collections::BTreeMap;

/// The listed trait a `dyn path` layout name is the table of.
fn table<'a>(rule: &Rule, reason: &'a str) -> Option<&'a str> {
    let path = reason.strip_prefix("dyn ")?;
    rule.trait_decl(path).map(|_| path)
}

/// The trait and method a `dyn path::method` layout name is the callable of.
fn method<'a>(rule: &Rule, reason: &'a str) -> Option<(&'a str, &'a str)> {
    let (path, name) = reason.strip_prefix("dyn ")?.rsplit_once("::")?;
    rule.trait_decl(path).map(|_| (path, name))
}

/// Finite metadata graph, with transparent references and no native engine values.
#[derive(Default)]
pub struct Graph {
    /// Descriptor-indexed layouts for `exposure::Plan::build`.
    pub layouts: Vec<Layout>,
    ids: BTreeMap<String, usize>,
}
impl Graph {
    /// Every layout a helper module can emit: named and not opaque.
    pub fn emittable(&self, rule: &Rule) -> Vec<bool> {
        self.layouts
            .iter()
            .map(|layout| layout.opaque.is_none() && rule.named(&layout.name).is_some())
            .collect()
    }
    /// Auxiliary node ID for a variant payload, map entry or UTF-16 unit.
    pub fn auxiliary(&self, shape: &Shape, role: &str) -> Option<usize> {
        self.id(shape).and_then(|id| {
            self.ids
                .get(&format!("{};{role}", self.layouts[id].name))
                .copied()
        })
    }
    /// Look up a previously added shape, transparently following references and wrappers.
    pub fn id(&self, shape: &Shape) -> Option<usize> {
        match shape {
            Shape::Ref { to, .. } | Shape::Wrapped(_, to) => self.id(to),
            Shape::Named(path, _) => self.ids.get(path).copied(),
            _ => self.ids.get(&format!("{shape:?}")).copied(),
        }
    }
    /// Emit metadata owned by registration, using the runtime crate's descriptor types.
    /// Build this once per source, not per read or per mod callback.
    pub fn tokens(&self) -> proc_macro2::TokenStream {
        let layouts = self.layouts.iter().map(|l| {
            let name = &l.name;
            let signature = &l.signature;
            let opaque = match &l.opaque {
                Some(reason) => quote!(Some(String::from(#reason))),
                None => quote!(None),
            };
            let children = &l.children;
            let fields = l.fields.iter().map(|f| {
                let case = match f.case {
                    Some(case) => quote!(Some(#case)),
                    None => quote!(None),
                };
                let ordinal = f.ordinal;
                let name = &f.name;
                let target = f.target;
                quote!(mod_points::exposure::Field { case: #case, ordinal: #ordinal, name: String::from(#name), target: #target })
            });
            quote!(mod_points::exposure::Layout {
                name: String::from(#name), signature: String::from(#signature),
                fields: vec![#(#fields),*], children: vec![#(#children),*], opaque: #opaque,
            })
        });
        quote!(vec![#(#layouts),*])
    }

    /// Add a reachable root, returning its source-local descriptor ID. Unsupported layouts are
    /// recorded as opaque and refused only if the registration policy actually reaches them.
    pub fn add(&mut self, rule: &Rule, shape: &Shape) -> usize {
        if let Shape::Ref { to, .. } | Shape::Wrapped(_, to) = shape {
            return self.add(rule, to);
        }
        let name = match shape {
            Shape::Named(path, _) => path.clone(),
            _ => format!("{shape:?}"),
        };
        if let Some(id) = self.ids.get(&name) {
            return *id;
        }
        let id = self.layouts.len();
        self.ids.insert(name.clone(), id);
        self.layouts.push(Layout {
            name,
            signature: String::new(),
            fields: Vec::new(),
            children: Vec::new(),
            opaque: None,
        });
        let mut fields = Vec::new();
        let mut children = Vec::new();
        let mut signature = format!("{shape:?}");
        let mut opaque = None;
        match shape {
            Shape::Named(path, _) => match rule.named(path) {
                None => opaque = Some("missing layout".into()),
                Some(named) => {
                    // The parsed declaration carries widths, case order, and member spellings.
                    let item = &named.item;
                    signature = quote!(#item).to_string();
                    if named.guarded
                        || named.generic
                        || named.opaque_reason.is_some()
                        || crate::crossing::foreign(path) == Some(crate::crossing::Foreign::Bits)
                    {
                        opaque = Some(named.opaque_reason.unwrap_or("inaccessible layout").into());
                    }
                    if named.foreign
                        && let syn::Item::Struct(s) = item
                        && s.fields
                            .iter()
                            .any(|f| !matches!(f.vis, syn::Visibility::Public(_)))
                    {
                        opaque = Some("private foreign fields".into());
                    }
                    let mut ordinals = BTreeMap::<Option<u32>, u32>::new();
                    for f in rule.fields(path) {
                        let case = f.case.map(|c| c.0);
                        let ordinal = ordinals.entry(case).or_default();
                        let member = &f.member;
                        fields.push(Field {
                            case,
                            ordinal: *ordinal,
                            name: quote!(#member).to_string(),
                            target: self.add(rule, &rule.shape(&named.module, Some(path), f.ty)),
                        });
                        *ordinal += 1;
                    }
                }
            },
            Shape::Tuple(parts) => children.extend(parts.iter().map(|p| self.add(rule, p))),
            Shape::Array(p, _) | Shape::Slice(p) | Shape::Option(p) => {
                children.push(self.add(rule, p))
            }
            Shape::Result(ok, err) => {
                children.push(self.add(rule, ok));
                children.push(self.add(rule, err));
            }
            Shape::Collection(kind, args) => {
                let count = if matches!(kind, Collection::HashMap | Collection::BTreeMap) {
                    2
                } else {
                    1
                };
                if args.len() < count {
                    opaque = Some("malformed collection".into());
                }
                children.extend(args.iter().take(count).map(|p| self.add(rule, p)));
            }
            // A listed trait's object is a table with one leaf per method, in declaration
            // order; a method that cannot cross is an opaque leaf with its reason.
            Shape::Opaque(Some(reason)) if table(rule, reason).is_some() => {
                let path = table(rule, reason).expect("checked");
                for (ordinal, method) in crate::native_callable::methods(rule, path)
                    .iter()
                    .enumerate()
                {
                    fields.push(Field {
                        case: None,
                        ordinal: ordinal as u32,
                        name: method.name.to_string(),
                        target: self.add(
                            rule,
                            &Shape::Opaque(Some(format!("dyn {path}::{}", method.name))),
                        ),
                    });
                }
            }
            Shape::Opaque(Some(reason)) if method(rule, reason).is_some() => {
                let (path, name) = method(rule, reason).expect("checked");
                opaque = crate::native_callable::method_gap(rule, path, name);
            }
            Shape::Opaque(reason) => {
                opaque = Some(reason.clone().unwrap_or("opaque layout".into()))
            }
            Shape::Scalar(crate::crossing::Scalar::Signed("isize")) => {
                opaque = Some("isize has no Wire encoding".into())
            }
            _ => {}
        }
        self.layouts[id].signature = signature;
        self.layouts[id].fields = fields;
        self.layouts[id].children = children;
        self.layouts[id].opaque = opaque;
        if matches!(
            shape,
            Shape::Scalar(crate::crossing::Scalar::String | crate::crossing::Scalar::Str)
                | Shape::DynError
        ) {
            let unit = self.add(
                rule,
                &Shape::Scalar(crate::crossing::Scalar::Unsigned("u16")),
            );
            self.ids
                .insert(format!("{};unit", self.layouts[id].name), unit);
            self.layouts[id].children.push(unit);
        }
        let mut auxiliaries = Vec::new();
        match shape {
            Shape::Option(_) => {
                auxiliaries.push(("variant(0)".to_string(), Vec::new()));
                auxiliaries.push(("variant(1)".to_string(), self.layouts[id].children.clone()));
            }
            Shape::Result(..) => {
                let children = self.layouts[id].children.clone();
                auxiliaries.push(("variant(0)".to_string(), vec![children[0]]));
                auxiliaries.push(("variant(1)".to_string(), vec![children[1]]));
            }
            Shape::Collection(Collection::HashMap | Collection::BTreeMap, _) => {
                auxiliaries.push(("pair".to_string(), self.layouts[id].children.clone()))
            }
            Shape::Named(path, _) => {
                if let Some(named) = rule.named(path)
                    && let syn::Item::Enum(item) = &named.item
                {
                    for tag in 0..item.variants.len() {
                        let targets = vec![id];
                        auxiliaries.push((format!("variant({tag})"), targets));
                    }
                }
            }
            _ => {}
        }
        for (role, children) in auxiliaries {
            let name = format!("{};{role}", self.layouts[id].name);
            let aux = self.layouts.len();
            self.ids.insert(name.clone(), aux);
            self.layouts.push(Layout {
                name,
                signature: role,
                fields: Vec::new(),
                children,
                opaque: None,
            });
            self.layouts[id].children.push(aux);
        }
        id
    }
}
