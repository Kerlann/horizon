use super::resolve::{Resolved, resolve};
use super::*;
use crate::{crossing::Rule, internals::Internal, points::file_points};
use quote::{ToTokens, format_ident, quote};
use sha2::{Digest, Sha256};
use syn::{FnArg, Type, spanned::Spanned};

fn digest(text: String) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))[..16].to_owned()
}

/// Rewrite one source file. Missing targets are left for the coordinator to check across files;
/// every matched target must resolve its call, and overlapping declarations are refused.
pub fn rewrite(
    source: &str,
    module: &str,
    entries: &[Internal],
    declarations: &[Declaration],
) -> Result<(String, Vec<Site>), Error> {
    let parsed = syn::parse_file(source)?;
    let mut functions = Vec::new();
    file_points(module, &parsed.items, &|_| true, &mut functions);
    let rule = Rule::new(entries);
    let mut edits = BTreeMap::new();
    let mut claimed = BTreeMap::new();
    let mut sites = Vec::new();
    let lines: Vec<_> = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let offset = |at| crate::points::offset(source, &lines, at);
    for declaration in declarations {
        for found in functions
            .iter()
            .filter(|f| plain_path(&f.target()) == declaration.target)
        {
            let build = || -> Result<_, Error> {
                let calls = super::analysis::calls(found.body, &declaration.selector);
                let mut lexical = super::analysis::Scope::scan(found.sig, Some(found.body));
                if calls.is_empty() {
                    return Err(Cause::Missing("selector matched no call").into());
                }
                if calls.len() > 1 && declaration.ordinal.is_none() {
                    return Err(Cause::Ambiguous("ambiguous selector: supply an ordinal").into());
                }
                let ordinal = declaration.ordinal.unwrap_or(0);
                let expr = *calls
                    .get(ordinal)
                    .ok_or(Cause::Missing("ordinal matched no call"))?;
                let enclosing = entries
                    .iter()
                    .find(|e| {
                        plain_path(&e.path) == declaration.target
                            && matches!(e.kind, "method" | "fn")
                            && e.cfg.is_empty()
                    })
                    .ok_or(Cause::Missing(
                        "enclosing function is not in the internals catalog",
                    ))?;
                let Resolved {
                    callee,
                    signature,
                    receiver,
                    receiver_type,
                    arguments,
                    fields,
                    types,
                    output,
                } = resolve(expr, enclosing, found.sig, &lexical, entries, &rule)?;
                let name = format!(
                    "S{}",
                    digest(format!(
                        "{}|{}|{ordinal}",
                        declaration.target, declaration.selector
                    ))
                );
                let record = format_ident!("__{name}Args");
                let listener = format_ident!("__{name}Listener");
                let variant = format_ident!("{name}");
                let receiver_alias = format_ident!("__{name}Receiver");
                let output_alias = format_ident!("__{name}Output");
                let recv = lexical.fresh("__recv");
                let args = lexical.fresh("__args");
                let wrap = lexical.fresh("__wrap");
                let mut original = expr.clone();
                let arg_fields = fields
                    .iter()
                    .map(|f| syn::parse_quote!(#args.#f))
                    .collect::<Vec<Expr>>();
                match &mut original {
                    Expr::MethodCall(call) => {
                        *call.receiver = syn::parse_quote!(#recv);
                        call.args = arg_fields.into_iter().collect();
                    }
                    Expr::Call(call) => {
                        call.args = arg_fields.into_iter().collect();
                    }
                    _ => unreachable!(),
                }
                let binding = match (receiver, signature.receiver()) {
                    (Some(expr), Some(r)) => {
                        let referenced = matches!(expr, Expr::Path(path) if path.path.is_ident("self")) || found.sig.inputs.iter().any(|arg| matches!(arg, FnArg::Typed(arg) if matches!(&*arg.ty, Type::Reference(_)) && arg.pat.to_token_stream().to_string() == expr.to_token_stream().to_string()));
                        let place = if referenced {
                            quote!(*#expr)
                        } else {
                            quote!(#expr)
                        };
                        if r.mutability.is_some() {
                            quote!(let #recv = &mut #place;)
                        } else {
                            quote!(let #recv = &#place;)
                        }
                    }
                    _ => quote!(let #recv = ();),
                };
                let owning = module.split("::").next().expect("a module has a crate");
                let local =
                    |ty: proc_macro2::TokenStream| -> Result<proc_macro2::TokenStream, Error> {
                        ty.to_string()
                            .replace(&format!(":: {owning} ::"), "crate ::")
                            .parse()
                            .map_err(|_| Cause::Signature("invalid native type spelling").into())
                    };
                let receiver_type = local(receiver_type)?;
                let output = local(output)?;
                let mut alias_type: Type = syn::parse2(receiver_type.clone())?;
                if let Type::Reference(reference) = &mut alias_type {
                    reference.lifetime = Some(syn::parse_quote!('r));
                }
                let raw = quote! {
                    pub struct #record { #(pub #fields: #types,)* }
                    pub type #receiver_alias<'g, 'r> = #alias_type;
                    pub type #output_alias = #output;
                    pub type #listener = for<'g> fn(#receiver_type, &mut #record, &mut dyn FnMut(#receiver_type, &mut #record) -> #output) -> #output;
                };
                let replacement = quote! {
                    if ::mod_points::__private::SITES[::mod_points::SiteId::#variant as usize].load(::std::sync::atomic::Ordering::Relaxed) {
                        #binding
                        let mut #args = #record { #(#fields: #arguments,)* };
                        ::horizon_mod_api_macros::site!(#wrap, #variant, #listener, #receiver_alias, #record, #output_alias, #recv, #args, |#recv: #receiver_alias<'_, '_>, #args: &mut #record| #original)
                    } else { #expr }
                };
                let site = Site {
                    name,
                    target: declaration.target.clone(),
                    selector: declaration.selector.clone(),
                    ordinal,
                    hash: digest(format!(
                        "{}|{}|{}",
                        enclosing.signature,
                        callee.signature,
                        expr.to_token_stream()
                    )),
                    listener: format!("{}::{listener}", found.module),
                    record: format!("{}::{record}", found.module),
                    receiver: receiver_type.to_string(),
                    args: fields
                        .iter()
                        .zip(&types)
                        .map(|(f, t)| (f.to_string(), t.to_token_stream().to_string()))
                        .collect(),
                    output: output.to_string(),
                };
                Ok((expr.span(), raw.to_string(), replacement.to_string(), site))
            };
            let (span, raw, replacement, site) = build()
                .map_err(|e| e.at(format!("{}: {}", declaration.origin, declaration.target)))?;
            let start = offset(span.start());
            let end = offset(span.end());
            if let Some(other) = claimed.insert((start, end), declaration.origin.clone()) {
                return Err(Error::from(Cause::Conflict(other)).at(declaration.origin.clone()));
            }
            edits.insert((start, end), replacement);
            edits
                .entry((offset(found.end), offset(found.end)))
                .or_insert_with(String::new)
                .push_str(&raw);
            sites.push(site);
        }
    }
    let mut out = source.to_owned();
    let mut boundary = source.len();
    for ((start, end), text) in edits.into_iter().rev() {
        if end > boundary {
            return Err(Cause::Record("nested selected call sites overlap").into());
        }
        out.replace_range(start..end, &text);
        boundary = start;
    }
    Ok((out, sites))
}

#[cfg(test)]
mod tests;
