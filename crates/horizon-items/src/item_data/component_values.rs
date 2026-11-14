//! Shared persistent component parsing and borrowed decoded-value comparisons.
use crate::properties::resolvable;
use horizon_client_core::registries::Registries;
use horizon_core::nbt::Tag;

/// Decode `Identifier.CODEC` without requiring a registered resource.
pub fn identifier(value: &Tag) -> Result<Tag, String> {
    if !matches!(value, Tag::String(_)) {
        return Err("Not a string".into());
    }
    resolvable::identifier(value)
        .map(resolvable::Identifier::to_tag)
        .ok_or_else(|| "Not a valid resource location".into())
}

pub(crate) fn identifier_equal(a: Option<&Tag>, b: Option<&Tag>) -> bool {
    match (
        a.and_then(resolvable::identifier),
        b.and_then(resolvable::identifier),
    ) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// `HolderSetCodec`: a named tag or an ordered direct list of registry references.
/// A single reference is the compact spelling of a one-element direct list.
enum HolderSet<'a> {
    Named(resolvable::Identifier<'a>),
    Direct(&'a [Tag]),
}

fn holders(value: &Tag) -> Option<HolderSet<'_>> {
    match value {
        Tag::String(name) if name.0.first() == Some(&u16::from(b'#')) => {
            resolvable::identifier_units(&name.0[1..]).map(HolderSet::Named)
        }
        value @ Tag::String(_) => Some(HolderSet::Direct(std::slice::from_ref(value))),
        Tag::List(values) => Some(HolderSet::Direct(values)),
        // NbtOps exposes every CollectionTag as a list, including primitive arrays.
        Tag::ByteArray(values) if values.is_empty() => Some(HolderSet::Direct(&[])),
        Tag::IntArray(values) if values.is_empty() => Some(HolderSet::Direct(&[])),
        Tag::LongArray(values) if values.is_empty() => Some(HolderSet::Direct(&[])),
        _ => None,
    }
}

fn registered(value: &Tag, names: &[String]) -> bool {
    let Tag::String(value) = value else {
        return false;
    };
    names
        .iter()
        .any(|name| name.encode_utf16().eq(value.0.iter().copied()))
}

/// `HolderSetCodec` with `RegistryFixedCodec` and compact singleton spelling.
/// Registry access includes the load's pack/mod entries; inline values are refused.
pub fn holder_set(value: &Tag, registry_key: &str, registry: &Registries) -> Result<Tag, String> {
    match holders(value).ok_or("Expected a registry reference, list of references, or #tag")? {
        HolderSet::Named(name) => {
            let name = name.to_tag();
            if !registered(&name, registry.tags(registry_key)) {
                let Tag::String(name) = name else {
                    unreachable!("encoded identifier")
                };
                return Err(format!(
                    "Missing tag: '{}' in '{registry_key}'",
                    name.to_string_lossy()
                ));
            }
            let Tag::String(mut name) = name else {
                unreachable!("encoded identifier")
            };
            name.0.insert(0, u16::from(b'#'));
            Ok(Tag::String(name))
        }
        HolderSet::Direct(values) => Ok(Tag::List(
            values
                .iter()
                .map(|value| {
                    let value = identifier(value)?;
                    if !registered(&value, registry.ids(registry_key)) {
                        let Tag::String(name) = value else {
                            unreachable!("encoded identifier")
                        };
                        return Err(format!(
                            "Failed to get element {} from registry {registry_key}",
                            name.to_string_lossy()
                        ));
                    }
                    Ok(value)
                })
                .collect::<Result<_, String>>()?,
        )),
    }
}

/// Named sets compare by registry tag identity; direct lists preserve order/duplicates.
pub fn holder_set_equal(a: &Tag, b: &Tag) -> bool {
    match (holders(a), holders(b)) {
        (Some(HolderSet::Named(a)), Some(HolderSet::Named(b))) => a == b,
        (Some(HolderSet::Direct(a)), Some(HolderSet::Direct(b))) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(a, b)| identifier_equal(Some(a), Some(b)))
        }
        _ => false,
    }
}
