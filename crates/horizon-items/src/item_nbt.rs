//! Item stacks in NBT (`ItemStack.CODEC`): id, count and components, alone or in
//! slot lists.
use horizon_client_core::nbt_value::{number, string, text};
use horizon_core::nbt::Tag;
use std::sync::Arc;

pub fn encode_item(stack: &crate::items::ItemStack) -> Vec<(&'static str, Tag)> {
    let mut entries = vec![
        ("id", string(&stack.item)),
        ("count", Tag::Int(stack.count as i32)),
    ];
    if let Some(components) = crate::item_data::encode_components(stack) {
        entries.push(("components", components));
    }
    entries
}
/// ItemStack.CODEC (id, count, components).
pub fn item_to_nbt(stack: &crate::items::ItemStack) -> Tag {
    Tag::compound(encode_item(stack))
}
/// ItemStack.CODEC saved against the actual loaded item prototype.
pub fn item_to_nbt_with_items(stack: &crate::items::ItemStack, items: &crate::items::Items) -> Tag {
    let mut entries = vec![
        ("id", string(&stack.item)),
        ("count", Tag::Int(stack.count as i32)),
    ];
    if let Some(components) = crate::item_data::encode_components_with_items(stack, items) {
        entries.push(("components", components));
    }
    Tag::compound(entries)
}
/// ItemStack.CODEC's count range is independent of the item's stack limit;
/// an out-of-range count uses the field's default of one. The id is read as an
/// `Identifier`: `stone` is `minecraft:stone` (a summoned display's or a
/// command's NBT often leaves the namespace out), and an id that is no valid
/// identifier makes no stack.
pub fn item_from_nbt(tag: &Tag) -> Option<crate::items::ItemStack> {
    let (namespace, path) = horizon_world::datafix::identifier(&text(tag.get("id"))?)?;
    let id = format!("{namespace}:{path}");
    if id == "minecraft:air" {
        return None;
    }
    let count = number(tag.get("count"))
        .or_else(|| number(tag.get("Count")))
        .filter(|count| (1..=99).contains(count))
        .unwrap_or(1) as u32;
    let (damage, extra) = crate::item_data::decode_components(tag.get("components"));
    Some(crate::items::ItemStack {
        item: Arc::from(id.as_str()),
        count,
        damage,
        extra,
    })
}
pub fn encode_items(slots: &[Option<crate::items::ItemStack>]) -> Tag {
    Tag::List(
        slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let mut entries = vec![("Slot", Tag::Byte(i as i8))];
                entries.extend(encode_item(s.as_ref()?));
                Some(Tag::compound(entries))
            })
            .collect(),
    )
}
pub fn decode_items(tag: Option<&Tag>, size: usize) -> Vec<Option<crate::items::ItemStack>> {
    let mut slots = vec![None; size];
    if let Some(Tag::List(items)) = tag {
        for it in items {
            let slot = number(it.get("Slot")).unwrap_or(0) as u8 as usize;
            if let Some(cell) = slots.get_mut(slot) {
                *cell = item_from_nbt(it);
            }
        }
    }
    slots
}

/// A stack as its saved NBT (`item_to_nbt`).
impl From<&crate::items::ItemStack> for Tag {
    fn from(stack: &crate::items::ItemStack) -> Self {
        item_to_nbt(stack)
    }
}

/// A stack read from its saved NBT (`item_from_nbt`); a tag that is no stack is refused as
/// itself.
impl TryFrom<Tag> for crate::items::ItemStack {
    type Error = Tag;
    fn try_from(tag: Tag) -> Result<Self, Tag> {
        item_from_nbt(&tag).ok_or(tag)
    }
}

/// Components as a stack's `components` compound (`components_tag`), undamaged.
impl From<&crate::item_data::StackData> for Tag {
    fn from(data: &crate::item_data::StackData) -> Self {
        crate::item_data::components_tag(0, Some(data))
            .unwrap_or_else(|| Tag::Compound(Default::default()))
    }
}

/// Components read from a stack's `components` compound (`decode_components`); a damage in it
/// is not a component of these.
impl TryFrom<Tag> for crate::item_data::StackData {
    type Error = Tag;
    fn try_from(tag: Tag) -> Result<Self, Tag> {
        let (_, extra) = crate::item_data::decode_components(Some(&tag));
        Ok(extra.map(|data| *data).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nbt_count_defaults_to_one_outside_codec_range() {
        for item in ["minecraft:stone", "minecraft:diamond_sword"] {
            for (raw, expected) in [
                (None, 1),
                (Some(Tag::Int(-1)), 1),
                (Some(Tag::Int(0)), 1),
                (Some(Tag::Int(1)), 1),
                (Some(Tag::Int(2)), 2),
                (Some(Tag::Int(64)), 64),
                (Some(Tag::Int(99)), 99),
                (Some(Tag::Int(100)), 1),
                (Some(Tag::Int(255)), 1),
                (Some(Tag::Int(256)), 1),
                (Some(Tag::Int(i32::MAX)), 1),
                (Some(Tag::Byte(1)), 1),
                (Some(Tag::Float(1.5_f32.to_bits())), 1),
                (Some(string("1")), 1),
            ] {
                let mut tag = Tag::compound([("id", string(item))]);
                if let (Tag::Compound(map), Some(raw)) = (&mut tag, &raw) {
                    map.insert("count".into(), raw.clone());
                }
                let stack = item_from_nbt(&tag).unwrap();
                assert_eq!(stack.count, expected, "{item}: {raw:?}");
                assert_eq!(
                    number(item_to_nbt(&stack).get("count")),
                    Some(i64::from(expected))
                );
            }
        }
    }

    /// `/summon item_display ~ ~ ~ {item:{id:"stone"}}` shows stone in vanilla:
    /// the id reads as an Identifier, with the default namespace (#8570).
    #[test]
    fn an_id_without_a_namespace_is_a_minecraft_item() {
        let read = |id: &str| item_from_nbt(&Tag::compound([("id", string(id))]));
        assert_eq!(
            read("stone").map(|s| s.item),
            Some("minecraft:stone".into())
        );
        assert_eq!(
            read(":diamond").map(|s| s.item),
            Some("minecraft:diamond".into())
        );
        assert_eq!(read("nexo:ruby").map(|s| s.item), Some("nexo:ruby".into()));
        assert!(read("air").is_none());
        assert!(read("Stone").is_none(), "not a valid identifier");
    }

    #[test]
    fn runtime_counts_remain_independent_of_the_nbt_decode_range() {
        let stack = crate::items::ItemStack::new("minecraft:diamond_sword", 1000);
        assert_eq!(stack.count, 1000);
        assert_eq!(number(item_to_nbt(&stack).get("count")), Some(1000));
    }
}
