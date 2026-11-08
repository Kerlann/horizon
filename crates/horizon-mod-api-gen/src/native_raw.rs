//! Rule-derived projection plans consumed by the point staging attribute.
//!
//! Plans contain native expressions, not native values or wire DTO definitions. The point
//! macro owns the exclusive input places and the private callback-owned value lease.
use crate::{crossing::Rule, native_patch, native_schema, native_window};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Signature, Type};

/// One phase's projection over the caller's tuple of borrowed native places.
pub struct Frame {
    /// Number of direct roots, excluding the virtual call node.
    pub roots: usize,
    /// Body of a checked PageSource reader, with `__raw_state` and `__raw_window` in scope.
    pub read: TokenStream,
    /// Body of record apply, after the private value lease has closed.
    pub apply: TokenStream,
}

/// A native projection plan. Return frames contain only the native return listener's outcome;
/// consumed by-value arguments are never read after the original function runs.
pub struct Plan {
    pub head: Frame,
    pub tail: Option<Frame>,
}

/// Build shared reads and locator-aware atomic edit plans for free-function places.
/// All roots share one patch tree validation and one final commit.
/// Receivers require the loader's manifest-selected field policy, never an implicit full view.
pub fn generate(
    rule: &Rule,
    module: &str,
    signature: &Signature,
    shape: &point_shape::Shape,
) -> Result<Plan, String> {
    if signature.receiver().is_some() {
        return Err("receiver requires the manifest-selected field policy".into());
    }
    if shape.borrowed_return {
        return Err("borrowed-return projection integration pending".into());
    }
    if matches!(shape.returns, point_shape::Returns::UnitResult) {
        return Err("UnitResult needs its distinct return Result projection".into());
    }
    let authority = point_shape::raw_shape(shape, signature, None);
    let mut inputs = Vec::new();
    for (argument, permission) in signature.inputs.iter().zip(&authority.args) {
        let syn::FnArg::Typed(argument) = argument else {
            unreachable!()
        };
        inputs.push(((*argument.ty).clone(), permission.record_mutable));
    }
    let outcome = shape.raw_outcome(signature);
    let result: Option<Type> = match &outcome {
        point_shape::RawOutcome::Cancel => Some(syn::parse_quote!(bool)),
        point_shape::RawOutcome::Result(ty) => Some(syn::parse_quote!(Option<#ty>)),
        point_shape::RawOutcome::None => None,
    };
    if let Some(result) = result {
        inputs.push((result, authority.outcome_record_mutable == Some(true)));
    }
    let head = frame(rule, module, &inputs)?;
    let tail = if shape.has_return() {
        let result = match outcome {
            point_shape::RawOutcome::Result(ty) => {
                vec![(ty, authority.return_record_mutable)]
            }
            // Native unit return listeners have no mutable cancellation control.
            point_shape::RawOutcome::Cancel | point_shape::RawOutcome::None => Vec::new(),
        };
        Some(frame(rule, module, &result)?)
    } else {
        None
    };
    Ok(Plan { head, tail })
}

impl Plan {
    /// Staging-only metadata removed by the point macro before its ordinary shape is parsed.
    pub fn attribute(&self) -> TokenStream {
        let Frame { roots, read, apply } = &self.head;
        let tail = self
            .tail
            .as_ref()
            .map(|Frame { roots, read, apply }| quote!(, #roots, { #read }, { #apply }));
        quote!(#[horizon_raw_plan(#roots, { #read }, { #apply } #tail)])
    }
}

fn frame(rule: &Rule, module: &str, roots: &[(Type, bool)]) -> Result<Frame, String> {
    let mut visitors = Vec::new();
    let mut plans = Vec::new();
    let mut prefixes = Vec::new();
    let slots: Vec<_> = (0..roots.len())
        .map(|index| quote::format_ident!("__raw_slot_{index}"))
        .collect();
    let mut schema = String::from("raw-call(");
    for (index, (ty, writable)) in roots.iter().enumerate() {
        let member = syn::Index::from(index);
        let ordinal = index as u64;
        let field = index as u32;
        let resolved = rule.shape(module, None, ty);
        schema.push_str(&native_schema::identity(
            rule,
            &resolved,
            &native_schema::All,
        )?);
        schema.push(';');
        let node = native_window::generate(
            rule,
            module,
            None,
            ty,
            quote!(*__roots.#member),
            &native_schema::All,
        )?;
        visitors.push(quote! {
            #ordinal => __visitor(mod_points::locators::Step::Field(#field), #field, &#node),
        });
        prefixes.push(quote! {
            __paths.find(Some(0), mod_points::locators::Step::Field(#field))
                .ok_or(mod_api::wire::WireError::Invalid("missing raw root locator"))?,
        });
        let slot = &slots[index];
        plans.push(native_patch::generate(
            rule,
            module,
            None,
            ty,
            quote!(**#slot),
            quote!(&__trees[#member]),
            quote!(__plan),
            *writable,
            &native_schema::All,
        )?);
    }
    schema.push(')');
    let count = roots.len() as u64;
    let read = quote! {
        let __guard = __raw_state.0.try_borrow()
            .map_err(|_| mod_api::RowError::Unsupported)?;
        let __roots = &*__guard;
        let mut __paths = __raw_state.1.try_borrow_mut()
            .map_err(|_| mod_api::RowError::Unsupported)?;
        let __node = mod_points::native_nodes::Record {
            key: const { mod_points::snapshot::key(#schema) },
            count: #count,
            visit: |__index, __visitor: &mut mod_points::native_nodes::Visitor<'_>| match __index {
                #(#visitors)*
                _ => Err(mod_api::RowError::Bounds),
            },
        };
        mod_points::native_read::read_checked(&__node, 0, &mut __paths, __raw_window)
    };
    let apply = quote! {
        let __patch = mod_api::wire::Reader::decode::<mod_api::wire::RecordPatch>(__raw_bytes)?;
        let __paths = __raw_state.1.try_borrow()
            .map_err(|_| mod_api::wire::WireError::Invalid("raw locators are already borrowed"))?;
        // Check every source-local destination before borrowing any native place.
        let __trees = mod_points::native_patch::PatchTree::roots(&__paths, &[#(#prefixes)*], &__patch)?;
        let mut __roots = __raw_state.0.try_borrow_mut()
            .map_err(|_| mod_api::wire::WireError::Invalid("raw places are already borrowed"))?;
        let (#(#slots,)*) = &mut *__roots;
        let mut __plan = mod_api::wire::PatchPlan::default();
        #(#plans)*
        __plan.commit()
    };
    Ok(Frame {
        roots: roots.len(),
        read: local_paths(read, module)?,
        apply: local_paths(apply, module)?,
    })
}

// Canonical schema names remain unchanged; only native Rust paths in emitted expressions
// become local to their owning crate. Lower crates do not implicitly import themselves.
fn local_paths(tokens: TokenStream, module: &str) -> Result<TokenStream, String> {
    use syn::visit_mut::VisitMut;
    struct Local<'a>(&'a str);
    impl VisitMut for Local<'_> {
        fn visit_path_mut(&mut self, path: &mut syn::Path) {
            syn::visit_mut::visit_path_mut(self, path);
            if let Some(first) = path.segments.first_mut()
                && first.ident == self.0
            {
                first.ident = syn::Ident::new("crate", first.ident.span());
                path.leading_colon = None;
            }
        }
    }
    let mut block: syn::Block =
        syn::parse2(quote!({ #tokens })).map_err(|error| error.to_string())?;
    Local(module.split("::").next().unwrap_or(module)).visit_block_mut(&mut block);
    let statements = block.stmts;
    Ok(quote!(#(#statements)*))
}
