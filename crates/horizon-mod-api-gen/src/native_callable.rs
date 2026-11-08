//! Generate ordinary Wire codec closures for a native callable source.
use crate::{
    crossing::{Rule, Shape},
    native_value,
};
use proc_macro2::TokenStream;
use quote::quote;

/// Expressions accepted by `FunctionSource::new`: decode an argument tuple and encode its
/// result. The source checks for trailing data before invoking the native callable. Unsupported
/// non-owned or inaccessible shapes are reported by the same codec rules as native values.
pub struct CallableCodecs {
    /// Capture-free argument decoder expression.
    pub decode: TokenStream,
    /// Capture-free result encoder expression.
    pub encode: TokenStream,
}

/// Generate codecs for an ordered list of native parameters and one native result.
pub fn codecs(rule: &Rule, arguments: &[Shape], result: &Shape) -> Result<CallableCodecs, String> {
    let args = native_value::codec(rule, &Shape::Tuple(arguments.to_vec()))?;
    let result = native_value::codec(rule, result)?;
    let decode = args.decode;
    let encode = result.encode;
    Ok(CallableCodecs {
        decode: quote!(|input: &mut mod_api::wire::Reader<'_>| -> Result<_, mod_api::wire::WireError> { Ok(#decode) }),
        encode: quote!(|value, out: &mut mod_api::wire::Writer| { #encode }),
    })
}
