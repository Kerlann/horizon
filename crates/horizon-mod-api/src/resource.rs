//! NeoForge 26.3's transfer API (#41 §7): a resource is what a handler holds, without an amount
//! ([`ItemResource`]), and a [`ResourceHandler`] is the indexed contents of one holder, written
//! only inside a transaction.
use super::{
    Capability, Compound, Host, HostTransaction, ItemStack, ServerHost, Tag, Transaction,
    backend_of,
};
use horizon_world::nbt::JavaString;
use std::fmt;

/// A data component's id, `namespace:path` (`minecraft:damage`, `mymod:charge`), checked once
/// where a mod names it: what [`ItemResource`]'s component methods take, as NeoForge's take a
/// `DataComponentType`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
// exhaustive: its fields are private, so mods cannot build or destructure it
pub struct ComponentId(Box<str>);

impl ComponentId {
    /// `id` as a component id, or None where it isn't a namespaced identifier
    /// (`Identifier.parse`'s characters, a namespace and a path): `Damage`, `!minecraft:food`
    /// and `my mod:charge` aren't.
    pub fn new(id: &str) -> Option<Self> {
        horizon_world::identifier::Identifier::parse_namespaced(id)?;
        Some(Self(id.into()))
    }
    /// The id, as saved (`minecraft:damage`).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An enchantment effect component type (`EnchantmentEffectComponents`): what an enchantment's
/// `effects` are keyed by, and what [`ItemResource::has_enchantment_effect`] asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
// exhaustive: its fields are private, so mods cannot build or destructure it
pub struct EnchantmentEffect(&'static str);

impl EnchantmentEffect {
    /// `id` as an effect type (a mod's own, as `mymod:soulbound`), or None where it isn't a
    /// namespaced identifier.
    pub fn new(id: &'static str) -> Option<Self> {
        horizon_world::identifier::Identifier::parse_namespaced(id)?;
        Some(Self(id))
    }
    /// The id (`minecraft:prevent_armor_change`).
    pub fn as_str(self) -> &'static str {
        self.0
    }
}

/// `EnchantmentEffectComponents.PREVENT_ARMOR_CHANGE`: worn armour with it stays on outside
/// creative (Curse of Binding's effect, and any data pack enchantment's that carries it).
pub const PREVENT_ARMOR_CHANGE: EnchantmentEffect =
    EnchantmentEffect("minecraft:prevent_armor_change");

/// An item and its components, without a count, as NeoForge's `ItemResource`: what a slot holds.
/// An empty slot holds none.
#[derive(Clone)]
// exhaustive: its fields are private, so mods cannot build or destructure it
pub struct ItemResource(pub(crate) ItemStack);

impl ItemResource {
    /// What `stack` is, whatever its count.
    pub fn of(stack: &ItemStack) -> Self {
        Self(ItemStack {
            count: 1,
            ..stack.clone()
        })
    }
    /// The item's id (`minecraft:apple`).
    pub fn item(&self) -> &str {
        &self.0.item
    }
    /// `count` of it as a stack.
    pub fn to_stack(&self, count: u32) -> ItemStack {
        ItemStack {
            count,
            ..self.0.clone()
        }
    }
    /// Whether `stack` is this item with these components.
    pub fn matches(&self, stack: &ItemStack) -> bool {
        self.0.same_item_same_components(stack)
    }
    /// This item's own value of component `id` in its patch, as saved (`minecraft:damage` on a
    /// worn sword); none where it keeps the item's default or has the component removed.
    pub fn component(&self, id: &ComponentId) -> Option<Tag> {
        horizon_items::item_data::encode_components(&self.0)?
            .get(id.as_str())
            .cloned()
    }
    /// This item's value of component `id` (`ItemResource.get`): its own, else the item's default
    /// (`host.item_default_component`), none where it has the component removed or the item
    /// hasn't one.
    pub fn value(&self, host: &dyn Host, id: &ComponentId) -> Option<Tag> {
        if let Some(patch) = horizon_items::item_data::encode_components(&self.0) {
            if let Some(own) = patch.get(id.as_str()) {
                return Some(own.clone());
            }
            if patch.get(&format!("!{}", id.as_str())).is_some() {
                return None;
            }
        }
        host.item_default_component(self.item(), id.as_str())
    }
    /// Whether an enchantment on this item has effect `effect` (`EnchantmentHelper.has`), by the
    /// enchantments' definitions, data packs' included, not their ids.
    pub fn has_enchantment_effect(&self, host: &dyn Host, effect: EnchantmentEffect) -> bool {
        backend_of(host)
            .is_some_and(|backend| backend.item_has_enchantment_effect(&self.0, effect.as_str()))
    }
    /// This item with component `id` set to `value` (`ItemResource.with`, as vanilla's
    /// `PatchedDataComponentMap.set`): a removal of it dropped, and the stack's own value too
    /// where `value` is the item's default (`host.item_default_component`), so the stack is
    /// the plain item again. A stack changes where it lives by an exchange of one of it for
    /// this: `place.exchange(host, tx, &changed, 1)`, which splits it off a bigger stack, in the
    /// transaction.
    pub fn with_component(&self, host: &dyn Host, id: &ComponentId, value: Tag) -> Self {
        let default = host.item_default_component(self.item(), id.as_str());
        let own = !default.is_some_and(|default| same_value(id, &default, &value));
        self.patched(id, own.then(|| (JavaString::from(id.as_str()), value)))
    }
    /// This item without component `id` (`ItemResource.without`, as vanilla's
    /// `PatchedDataComponentMap.remove`): its own value dropped and, where the item has the
    /// component by default (`host.item_default_component`), a removal recorded in its patch
    /// (`"!minecraft:max_damage": {}`), saved and sent as vanilla's are.
    pub fn without_component(&self, host: &dyn Host, id: &ComponentId) -> Self {
        let removal = host
            .item_default_component(self.item(), id.as_str())
            .is_some()
            .then(|| {
                (
                    JavaString::from(format!("!{}", id.as_str()).as_str()),
                    Tag::Compound(Compound::new()),
                )
            });
        self.patched(id, removal)
    }
    /// This item with its patch's entries for `id`, a value or a removal, replaced by `entry`.
    fn patched(&self, id: &ComponentId, entry: Option<(JavaString, Tag)>) -> Self {
        let mut components = match horizon_items::item_data::encode_components(&self.0) {
            Some(Tag::Compound(components)) => components,
            _ => Compound::new(),
        };
        components.remove(&JavaString::from(id.as_str()));
        components.remove(&JavaString::from(format!("!{}", id.as_str()).as_str()));
        if let Some((key, value)) = entry {
            components.insert(key, value);
        }
        let (damage, extra) =
            horizon_items::item_data::decode_components(Some(&Tag::Compound(components)));
        Self(ItemStack {
            item: self.0.item.clone(),
            count: 1,
            damage,
            extra,
        })
    }
}

/// Components the game keeps as the NBT given (`CustomData`, `TypedEntityData`), equal only
/// type for type (`CompoundTag.equals`).
const RAW_NBT: [&str; 4] = [
    "minecraft:custom_data",
    "minecraft:bucket_entity_data",
    "minecraft:entity_data",
    "minecraft:block_entity_data",
];

/// Whether `a` and `b` are one value of component `id` (`Objects.equals` on the decoded
/// values, `PatchedDataComponentMap.set`). A vanilla component other than [`RAW_NBT`] is
/// compared as its codec reads the item report's JSON beside a mod's NBT ([`same_coded`]);
/// a raw NBT one, or a mod's, whose codec the game doesn't know, type for type.
fn same_value(id: &ComponentId, a: &Tag, b: &Tag) -> bool {
    let coded = id.as_str().starts_with("minecraft:") && !RAW_NBT.contains(&id.as_str());
    if coded { same_coded(a, b) } else { a == b }
}

/// Whether `a` and `b` read as one value through a codec (`NbtOps`): integers of any width
/// by exact value, a `Float` beside any number at `f32` precision and a `Double` at `f64`,
/// an array as the list of its numbers, and lists and compounds entry by entry.
fn same_coded(a: &Tag, b: &Tag) -> bool {
    enum Number {
        Integer(i64),
        Float(f32),
        Double(f64),
    }
    let number = |tag: &Tag| match *tag {
        Tag::Byte(v) => Some(Number::Integer(v.into())),
        Tag::Short(v) => Some(Number::Integer(v.into())),
        Tag::Int(v) => Some(Number::Integer(v.into())),
        Tag::Long(v) => Some(Number::Integer(v)),
        Tag::Float(bits) => Some(Number::Float(f32::from_bits(bits))),
        Tag::Double(bits) => Some(Number::Double(f64::from_bits(bits))),
        _ => None,
    };
    if let (Some(a), Some(b)) = (number(a), number(b)) {
        let float = |n: &Number| match *n {
            Number::Integer(v) => v as f32,
            Number::Float(v) => v,
            Number::Double(v) => v as f32,
        };
        let double = |n: &Number| match *n {
            Number::Integer(v) => v as f64,
            Number::Float(v) => v.into(),
            Number::Double(v) => v,
        };
        return match (&a, &b) {
            (Number::Integer(a), Number::Integer(b)) => a == b,
            (Number::Float(_), _) | (_, Number::Float(_)) => float(&a) == float(&b),
            _ => double(&a) == double(&b),
        };
    }
    let list = |tag: &Tag| -> Option<Vec<Tag>> {
        Some(match tag {
            Tag::List(items) => items.clone(),
            Tag::ByteArray(items) => items.iter().map(|&v| Tag::Byte(v as i8)).collect(),
            Tag::IntArray(items) => items.iter().map(|&v| Tag::Int(v)).collect(),
            Tag::LongArray(items) => items.iter().map(|&v| Tag::Long(v)).collect(),
            _ => return None,
        })
    };
    if let (Some(a), Some(b)) = (list(a), list(b)) {
        return a.len() == b.len() && a.iter().zip(&b).all(|(a, b)| same_coded(a, b));
    }
    match (a, b) {
        (Tag::Compound(a), Tag::Compound(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| same_coded(a, b)))
        }
        _ => a == b,
    }
}

impl PartialEq for ItemResource {
    fn eq(&self, other: &Self) -> bool {
        self.matches(&other.0)
    }
}
impl Eq for ItemResource {}

impl fmt::Debug for ItemResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ItemResource").field(&self.0.item).finish()
    }
}

/// The contents of one holder (a block entity as one side reaches it, an item where it lives),
/// numbered from 0, as NeoForge's `ResourceHandler`: a provider makes one for its holder, so it
/// knows where its contents live. It holds no borrow of the game: each call is given the host,
/// and its state is the game's (slots, a saved compound, attachments, a stack's components), so
/// the transaction `tx` a write runs in puts it back on an abort. Amounts never exceed what was
/// asked; a handler answering more is a mod bug, and the move it was part of is aborted.
pub trait ResourceHandler<R> {
    /// How many indices it has.
    fn size(&self, host: &mut dyn ServerHost) -> usize;
    /// What index `index` holds, if anything.
    fn resource(&self, host: &mut dyn ServerHost, index: usize) -> Option<R>;
    /// How much of it.
    fn amount(&self, host: &mut dyn ServerHost, index: usize) -> u32;
    /// How much of `resource` index `index` holds when it holds that, whatever it holds now
    /// (`getCapacityAsLong`). Capacity is not free space or permission to insert: validity,
    /// current contents and access rules can still refuse it.
    fn capacity(&self, host: &mut dyn ServerHost, index: usize, resource: &R) -> u32;
    /// Whether `resource` may go in index `index` at all (`isValid`). The default uses capacity;
    /// a handler with separate acceptance rules overrides this and enforces them in `insert`.
    fn is_valid(&self, host: &mut dyn ServerHost, index: usize, resource: &R) -> bool {
        self.capacity(host, index, resource) > 0
    }
    /// Puts up to `amount` of `resource` into index `index`: how much went in.
    fn insert(
        &self,
        host: &mut dyn ServerHost,
        tx: &Transaction,
        index: usize,
        resource: &R,
        amount: u32,
    ) -> u32;
    /// Takes up to `amount` of `resource` out of index `index`: how much came out. None of
    /// another resource.
    fn extract(
        &self,
        host: &mut dyn ServerHost,
        tx: &Transaction,
        index: usize,
        resource: &R,
        amount: u32,
    ) -> u32;
    /// Puts up to `amount` of `resource` in wherever it goes (NeoForge's `insert` without an
    /// index): by default index by index from 0. How much went in.
    fn insert_any(
        &self,
        host: &mut dyn ServerHost,
        tx: &Transaction,
        resource: &R,
        amount: u32,
    ) -> u32 {
        let mut went = 0u32;
        for index in 0..self.size(host) {
            if went >= amount {
                break;
            }
            went = went.saturating_add(self.insert(host, tx, index, resource, amount - went));
        }
        went
    }
    /// Takes up to `amount` of `resource` out from wherever it is (`extract` without an index):
    /// by default index by index from 0. How much came out.
    fn extract_any(
        &self,
        host: &mut dyn ServerHost,
        tx: &Transaction,
        resource: &R,
        amount: u32,
    ) -> u32 {
        let mut came = 0u32;
        for index in 0..self.size(host) {
            if came >= amount {
                break;
            }
            came = came.saturating_add(self.extract(host, tx, index, resource, amount - came));
        }
        came
    }
}

/// A handler answered more than it was asked (`move_resources`): a mod bug, warned once naming
/// the mod. Returned from a `host.transaction` closure with `?`, it aborts the whole move, so
/// neither end keeps any of it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Overanswered {
    /// The mod whose handler answered, or "the game".
    pub owner: String,
}

/// Moves up to `max` of what `filter` accepts out of `from` into wherever `to` takes it, index by
/// index from 0 (NeoForge's `ResourceHandlerUtil.move`), in transaction `tx`: how much moved.
/// Each move is all or nothing, in a transaction of its own: what `to` won't take stays in
/// `from`, and a move `to` takes only part of is tried again with that part (a cauldron gives
/// whole thirds). A handler answering more than it was asked has its move undone and ends the
/// moving with [`Overanswered`].
pub fn move_resources<R>(
    host: &mut dyn ServerHost,
    from: &Capability<dyn ResourceHandler<R>>,
    to: &Capability<dyn ResourceHandler<R>>,
    filter: impl Fn(&R) -> bool,
    max: u32,
    tx: &Transaction,
) -> Result<u32, Overanswered> {
    let _ = tx;
    let mut moved = 0;
    for index in 0..from.size(host) {
        while moved < max {
            let Some(resource) = from.resource(host, index).filter(|r| filter(r)) else {
                break;
            };
            let want = (max - moved).min(from.amount(host, index));
            let went = move_once(
                host,
                Ends {
                    from,
                    to,
                    at: &index,
                },
                want,
                |host, tx, want| from.extract(host, tx, index, &resource, want),
                |host, tx, out| to.insert_any(host, tx, &resource, out),
            )?;
            if went == 0 {
                break;
            }
            moved += went;
        }
    }
    Ok(moved)
}

/// The two ends of a move and where it is, for warnings.
pub(super) struct Ends<'a, A: ?Sized, B: ?Sized> {
    pub from: &'a Capability<A>,
    pub to: &'a Capability<B>,
    pub at: &'a dyn fmt::Debug,
}

/// One move of up to `want` by `extract` out of `ends.from` and `insert` into `ends.to`, all or
/// nothing in a transaction of its own, nested in the caller's: how much moved. A move `to` takes
/// only part of is undone and tried again with that part; a handler answering more than it was
/// asked has its move undone and gives [`Overanswered`]. The one step of `move_resources` and
/// `move_energy`.
pub(super) fn move_once<A: ?Sized, B: ?Sized>(
    host: &mut dyn ServerHost,
    ends: Ends<'_, A, B>,
    want: u32,
    extract: impl Fn(&mut dyn ServerHost, &Transaction, u32) -> u32,
    insert: impl Fn(&mut dyn ServerHost, &Transaction, u32) -> u32,
) -> Result<u32, Overanswered> {
    let Ends { from, to, at } = ends;
    // `Err(Ok(took))`: `to` took only that much; `Err(Err(_))`: a handler answered more than it
    // was asked.
    let step = |host: &mut dyn ServerHost, want: u32| {
        host.transaction(|host, tx| {
            let Some(out) = from.checked(extract(host, tx, want), want, at) else {
                return Err(Err(Overanswered {
                    owner: from.owner(),
                }));
            };
            match to.checked(insert(host, tx, out), out, at) {
                None => Err(Err(Overanswered { owner: to.owner() })),
                Some(took) if took == out => Ok(out),
                Some(took) => Err(Ok(took)),
            }
        })
    };
    match step(host, want) {
        Err(Ok(took)) if took > 0 => step(host, took),
        answer => answer,
    }
    .or_else(|answer| answer.map(|_| 0))
}
