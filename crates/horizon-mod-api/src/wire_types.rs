//! Checked wire decoding for opaque API values.
use crate::wire::{Reader, Wire, WireError, Writer};
use crate::{
    BlockEntityTypeKey, BlockKey, BoolGameRuleKey, DimensionKey, EffectKey, FluidKey, ItemKey,
    TagKey, UnresolvedFluid,
};

impl Wire for crate::OwnProvider {
    const MIN: usize = 12;
    fn put(&self, out: &mut Writer) {
        out.put(&self.handler);
        if let Ok(id) = out.resource(Box::new(self.provider.clone())) {
            if id == crate::wire::Unsupported::ID {
                out.refused(&self.handler)
            }
            out.put(&id);
        }
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            handler: input.take()?,
            provider: input.take()?,
        })
    }
}

impl Wire for crate::Transaction {
    const MIN: usize = 0;
    fn put(&self, _: &mut Writer) {}
    fn take(_: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self(()))
    }
}
impl Wire for crate::TransactionSeal {
    const MIN: usize = 0;
    fn put(&self, _: &mut Writer) {}
    fn take(_: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self(()))
    }
}
impl Wire for crate::LoadedMods {
    const MIN: usize = 0;
    fn put(&self, _: &mut Writer) {}
    fn take(_: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new())
    }
}
impl Wire for crate::RegistrationContext {
    const MIN: usize = 0;
    fn put(&self, _: &mut Writer) {}
    fn take(_: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new())
    }
}
impl Wire for crate::PackageKind {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(
            &(match self {
                Self::Code => 0u32,
                Self::Content => 1,
            }),
        );
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        match input.take()? {
            0u32 => Ok(Self::Code),
            1 => Ok(Self::Content),
            _ => Err(WireError::Invalid("invalid package kind")),
        }
    }
}
impl Wire for crate::Side {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(
            &(match self {
                Self::Client => 0u32,
                Self::Both => 1,
            }),
        );
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        match input.take()? {
            0u32 => Ok(Self::Client),
            1 => Ok(Self::Both),
            _ => Err(WireError::Invalid("invalid package side")),
        }
    }
}
impl Wire for crate::PackageInfo {
    const MIN: usize = 24;
    fn put(&self, out: &mut Writer) {
        out.put(&self.id);
        out.put(&self.version);
        out.put(&self.kind);
        out.put(&self.side);
        out.put(&self.namespaces);
        out.put(&self.dependencies);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new(
            input.take()?,
            input.take()?,
            input.take()?,
            input.take()?,
            input.take()?,
            input.take()?,
        ))
    }
}

#[cfg(test)]
mod resource_wire_tests {
    use super::*;

    #[test]
    fn wire_tokens_and_package_inventory() {
        let mut out = Writer::new();
        out.put(&crate::Transaction(()));
        out.put(&crate::TransactionSeal(()));
        assert!(out.bytes().is_empty());
        let mut input = Reader::new(out.bytes());
        let _: crate::Transaction = input.take().unwrap();
        let _: crate::TransactionSeal = input.take().unwrap();
        crate::phase::with_test_packages(&["test_mod"], || {
            let mods: crate::LoadedMods = input.take().unwrap();
            let context: crate::RegistrationContext = input.take().unwrap();
            assert_eq!(mods.ids().collect::<Vec<_>>(), ["test_mod"]);
            assert_eq!(context.mods().ids().collect::<Vec<_>>(), ["test_mod"]);
        });
        let package = crate::PackageInfo::new(
            "test".into(),
            "2".into(),
            crate::PackageKind::Content,
            crate::Side::Both,
            vec!["test".into()],
            vec!["other".into()],
        );
        out.put(&package);
        let decoded: crate::PackageInfo = Reader::new(out.bytes()).take().unwrap();
        assert_eq!(decoded.id(), "test");
        assert_eq!(decoded.version(), "2");
        assert_eq!(decoded.kind(), crate::PackageKind::Content);
        assert_eq!(decoded.side(), crate::Side::Both);
        assert!(decoded.depends_on("other"));
        for end in 0..out.bytes().len() {
            assert!(
                Reader::new(&out.bytes()[..end])
                    .take::<crate::PackageInfo>()
                    .is_err()
            );
        }
        assert!(Reader::new(&[2, 0, 0, 0]).take::<crate::Side>().is_err());
        assert!(
            Reader::new(&[2, 0, 0, 0])
                .take::<crate::PackageKind>()
                .is_err()
        );
    }
}

macro_rules! slot_keys {
    ($width:ty; $($key:ty),+ $(,)?) => {$ (
        impl Wire for $key {
            const MIN: usize = <$width as Wire>::MIN;
            fn put(&self, out: &mut Writer) { out.put(&self.0); }
            fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
                let slot=input.take::<$width>()?;
                // Slots belong to the host's registry. A wasm guest carries the opaque
                // token back; its separate registry tables cannot validate a host slot.
                // Host decoding remains the authoritative validation boundary.
                #[cfg(target_arch="wasm32")]
                { Ok(Self(slot)) }
                #[cfg(not(target_arch="wasm32"))]
                { Self::from_slot(slot).map_err(WireError::Invalid) }
            }
        }
    )+};
}
slot_keys!(u32; BlockEntityTypeKey, BlockKey, BoolGameRuleKey, DimensionKey, EffectKey, FluidKey, ItemKey, crate::particle_keys::ParticleKey, TagKey);
slot_keys!(u64; UnresolvedFluid);

impl Wire for crate::NativeWorldEpoch {
    const MIN: usize = 8;
    fn put(&self, out: &mut Writer) {
        out.put(&self.get());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new(input.take()?))
    }
}
impl Wire for crate::NativeTickId {
    const MIN: usize = 8;
    fn put(&self, out: &mut Writer) {
        out.put(&self.get());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new(input.take()?))
    }
}
impl Wire for crate::EntityRef {
    const MIN: usize = 12;
    fn put(&self, out: &mut Writer) {
        out.put(&self.epoch());
        out.put(&self.id());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new(input.take()?, input.take()?))
    }
}
macro_rules! entity_handles {
    ($($ty:ty),+ $(,)?) => { $(
        impl Wire for $ty {
            const MIN: usize = crate::EntityRef::MIN;
            fn put(&self, out: &mut Writer) { out.put(&self.entity()); }
            fn take(input: &mut Reader<'_>) -> Result<Self, WireError> { Ok(Self::new(input.take()?)) }
        }
    )+ };
}
entity_handles!(
    crate::LivingRef,
    crate::MobRef,
    crate::DragonRef,
    crate::ItemEntityRef,
    crate::DisplayRef,
    crate::ProjectileRef,
    crate::FishingHookRef,
    crate::MerchantRef,
    crate::MinecartRef
);

impl Wire for crate::Identifier {
    const MIN: usize = 8;
    fn put(&self, out: &mut Writer) {
        out.put_str(self.namespace());
        out.put_str(self.path());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Self::from_parts(&input.take::<String>()?, &input.take::<String>()?)
            .map_err(|_| WireError::Invalid("invalid identifier"))
    }
}
impl Wire for crate::LevelKey {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put_str(&String::from(self));
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Self::try_from(input.take::<String>()?).map_err(|_| WireError::Invalid("invalid level key"))
    }
}
impl Wire for crate::ConfigPath {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put_str(self.as_str());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Self::new(&input.take::<String>()?).map_err(|_| WireError::Invalid("invalid config path"))
    }
}
impl Wire for crate::SpawnerKey {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put_str(self.as_str());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Self::new(&input.take::<String>()?).ok_or(WireError::Invalid("invalid spawner key"))
    }
}
impl Wire for crate::CustomSpawners {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(&Vec::from(self));
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Self::try_from(input.take::<Vec<crate::SpawnerEntry>>()?)
            .map_err(|_| WireError::Invalid("duplicate spawner"))
    }
}
impl Wire for crate::FileToIdConverter {
    const MIN: usize = 8;
    fn put(&self, out: &mut Writer) {
        out.put(&<(String, String)>::from(self));
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Self::try_from(input.take::<(String, String)>()?)
            .map_err(|_| WireError::Invalid("invalid file converter"))
    }
}
impl Wire for crate::ReloadResourceError {
    const MIN: usize = 12;
    fn put(&self, out: &mut Writer) {
        out.put(&<(crate::Identifier, String)>::from(self));
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::from(input.take::<(crate::Identifier, String)>()?))
    }
}
impl Wire for crate::BlockState {
    const MIN: usize = 6;
    fn put(&self, out: &mut Writer) {
        out.put(&self.block());
        out.put(&self.ordinal());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(crate::BlockStateRecord::new(input.take()?, input.take()?).into())
    }
}
impl Wire for crate::BlockStateRange {
    const MIN: usize = 10;
    fn put(&self, out: &mut Writer) {
        out.put(&self.first());
        out.put(&self.len());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Self::new(input.take()?, input.take()?)
            .map_err(|_| WireError::Invalid("invalid block state range"))
    }
}

/// An NBT key: UTF-16 units, in the bytes of a `Vec<u16>` but written and read in one pass.
impl Wire for horizon_core::nbt::JavaString {
    fn retained_bytes(&self) -> Option<usize> {
        self.0.retained_bytes()
    }
    fn take_into(&mut self, input: &mut Reader<'_>) -> Result<(), WireError> {
        input.nested(|input| self.0.take_into(input))
    }
    const MIN: usize = <Vec<u16> as Wire>::MIN;
    fn put(&self, out: &mut Writer) {
        out.put_u16s(&self.0);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        input.nested(|input| input.take_u16s().map(Self))
    }
}

impl Wire for crate::Components {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(self.as_nbt());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::from(input.take::<horizon_core::nbt::Compound>()?))
    }
}
impl Wire for crate::ItemStack {
    fn retained_bytes(&self) -> Option<usize> {
        std::mem::size_of::<Self>().checked_add(match self.components() {
            Some(components) => components.as_nbt().retained_bytes()?,
            None => 0,
        })
    }
    const MIN: usize = 10;
    fn put(&self, out: &mut Writer) {
        out.put(&self.item());
        out.put(&self.count());
        out.put(&self.damage_component());
        out.put(&self.components().cloned());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        let mut stack = Self::new(input.take()?, input.take()?)
            .ok_or(WireError::Invalid("empty item stack"))?;
        if let Some(damage) = input.take::<Option<u32>>()? {
            stack.set_damage(damage);
        }
        if let Some(components) = input.take::<Option<crate::Components>>()? {
            stack
                .set_components(components.as_nbt().clone())
                .map_err(|_| WireError::Invalid("invalid item components"))?;
        }
        Ok(stack)
    }
}

impl Wire for crate::SpawnerEntry {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        match self {
            Self::Builtin(v) => {
                out.put(&0u32);
                out.put(v)
            }
            Self::Mod(v) => {
                out.put(&1u32);
                out.put(v)
            }
        }
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(match input.take::<u32>()? {
            0 => Self::Builtin(input.take()?),
            1 => Self::Mod(input.take()?),
            n => return Err(WireError::Tag(n)),
        })
    }
}
impl Wire for crate::LevelRef {
    const MIN: usize = 12;
    fn put(&self, out: &mut Writer) {
        out.put(&self.epoch());
        out.put(&self.dimension());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new(input.take()?, input.take()?))
    }
}
impl Wire for crate::ChunkRef {
    const MIN: usize = 20;
    fn put(&self, out: &mut Writer) {
        out.put(&self.level());
        out.put(&self.pos());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new(input.take()?, input.take()?))
    }
}
impl Wire for crate::RaidRef {
    const MIN: usize = 16;
    fn put(&self, out: &mut Writer) {
        out.put(&self.level());
        out.put(&self.id());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new(input.take()?, input.take()?))
    }
}
impl Wire for crate::PlayerRef {
    const MIN: usize = 28;
    fn put(&self, out: &mut Writer) {
        out.put(&self.entity());
        out.put(&self.uuid());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(crate::__private::game::player_ref(
            input.take()?,
            input.take()?,
        ))
    }
}
impl Wire for crate::GlobalPos {
    const MIN: usize = 20;
    fn put(&self, out: &mut Writer) {
        out.put(self.dimension());
        out.put(&self.pos());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new(input.take()?, input.take()?))
    }
}
impl Wire for crate::SoundRef {
    const MIN: usize = 8;
    fn put(&self, out: &mut Writer) {
        out.put(&self.id);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self { id: input.take()? })
    }
}
impl Wire for crate::BossBarRef {
    const MIN: usize = 24;
    fn put(&self, out: &mut Writer) {
        out.put(&self.epoch);
        out.put(&self.uuid);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            epoch: input.take()?,
            uuid: input.take()?,
        })
    }
}
impl Wire for crate::MenuRef {
    const MIN: usize = 40;
    fn put(&self, out: &mut Writer) {
        out.put(&self.player);
        out.put(&self.window);
        out.put(&self.generation);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            player: input.take()?,
            window: input.take()?,
            generation: input.take()?,
        })
    }
}
impl Wire for crate::BlockEntityRef {
    const MIN: usize = 32;
    fn put(&self, out: &mut Writer) {
        out.put(&self.level);
        out.put(&self.position);
        out.put(&self.generation);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            level: input.take()?,
            position: input.take()?,
            generation: input.take()?,
        })
    }
}
impl Wire for crate::UnspawnedMobRef {
    const MIN: usize = 16;
    fn put(&self, out: &mut Writer) {
        out.put(&self.epoch());
        out.put(&self.lend());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new(input.take()?, input.take()?))
    }
}
impl Wire for crate::ModelRef {
    const MIN: usize = 12;
    fn put(&self, out: &mut Writer) {
        out.put(&self.0);
        out.put(&self.1);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Self::from_parts(input.take()?, input.take()?).map_err(WireError::Invalid)
    }
}
impl Wire for crate::InstanceGroupRef {
    const MIN: usize = 12;
    fn put(&self, out: &mut Writer) {
        out.put(&self.0);
        out.put(&self.1);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Self::from_parts(input.take()?, input.take()?).map_err(WireError::Invalid)
    }
}
impl Wire for crate::TicketController {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(&self.slot());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Self::from_slot(input.take()?).ok_or(WireError::Invalid("unknown ticket controller"))
    }
}

impl Wire for crate::SpawnerKind {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(&(*self as u32));
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(match input.take::<u32>()? {
            0 => Self::Phantom,
            1 => Self::Patrol,
            2 => Self::Cat,
            3 => Self::VillageSiege,
            4 => Self::WanderingTrader,
            n => return Err(WireError::Tag(n)),
        })
    }
}
impl Wire for crate::NativeBlockUpdateFlags {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(&self.bits());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self(input.take()?))
    }
}
impl Wire for crate::FeatureFlags {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(&self.iter().map(str::to_owned).collect::<Vec<_>>());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new(input.take::<Vec<String>>()?.into()))
    }
}
impl Wire for crate::NativeMobGoalEntry {
    const MIN: usize = 9;
    fn put(&self, out: &mut Writer) {
        out.put(&self.priority);
        out.put_str(self.id);
        out.put(&self.running);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            priority: input.take()?,
            id: crate::wire::intern(&input.take::<String>()?)?,
            running: input.take()?,
        })
    }
}
impl Wire for crate::BlockEntityPartError {
    const MIN: usize = 8;
    fn put(&self, out: &mut Writer) {
        match self {
            Self::VanillaType { kind } => {
                out.put(&0u32);
                out.put(kind);
            }
            Self::Duplicate { kind, part } => {
                out.put(&1u32);
                out.put(kind);
                out.put_str(part);
            }
        }
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(match input.take::<u32>()? {
            0 => Self::VanillaType {
                kind: input.take()?,
            },
            1 => Self::Duplicate {
                kind: input.take()?,
                part: crate::wire::intern(&input.take::<String>()?)?,
            },
            n => return Err(WireError::Tag(n)),
        })
    }
}
impl Wire for crate::ItemResource {
    const MIN: usize = crate::ItemStack::MIN;
    fn put(&self, out: &mut Writer) {
        out.put(&self.0);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::of(&input.take()?))
    }
}
impl Wire for crate::FluidResource {
    const MIN: usize = crate::FluidStack::MIN;
    fn put(&self, out: &mut Writer) {
        out.put(&self.0);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::of(&input.take()?))
    }
}
impl Wire for crate::Corners {
    const MIN: usize = 24;
    fn put(&self, out: &mut Writer) {
        out.put(&self.width);
        out.put(&self.height);
        out.put(&self.used);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            width: input.take()?,
            height: input.take()?,
            used: input.take()?,
        })
    }
}
impl Wire for crate::ParticleOptions {
    const MIN: usize = 45;
    fn put(&self, out: &mut Writer) {
        out.put(&self.ty);
        out.put(&self.block);
        out.put(&self.color);
        out.put(&self.scale);
        out.put(&self.to_color);
        out.put(&self.trail);
        out.put(&self.item);
        out.put(&self.geyser);
        out.put(&self.vibration);
        out.put(&self.legacy_entity_effect);
        out.put(&self.legacy_trail_lifetime);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            ty: input.take()?,
            block: input.take()?,
            color: input.take()?,
            scale: input.take()?,
            to_color: input.take()?,
            trail: input.take()?,
            item: input.take()?,
            geyser: input.take()?,
            vibration: input.take()?,
            legacy_entity_effect: input.take()?,
            legacy_trail_lifetime: input.take()?,
        })
    }
}
impl Wire for crate::Reduction {
    const MIN: usize = 1;
    fn put(&self, out: &mut Writer) {
        out.put(
            &(match self {
                Self::Invulnerability => 0u8,
                Self::Armor => 1,
                Self::Enchantments => 2,
                Self::MobEffects => 3,
                Self::Absorption => 4,
                Self::InnateResistance => 5,
            }),
        );
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(match input.take::<u8>()? {
            0 => Self::Invulnerability,
            1 => Self::Armor,
            2 => Self::Enchantments,
            3 => Self::MobEffects,
            4 => Self::Absorption,
            5 => Self::InnateResistance,
            _ => return Err(WireError::Invalid("invalid reduction kind")),
        })
    }
}
impl Wire for crate::ReductionChange {
    const MIN: usize = 5;
    fn put(&self, out: &mut Writer) {
        let (kind, value) = match *self {
            Self::Add(v) => (0u8, v),
            Self::Scale(v) => (1, v),
            Self::AtLeast(v) => (2, v),
            Self::AtMost(v) => (3, v),
        };
        out.put(&kind);
        out.put(&value);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        let kind: u8 = input.take()?;
        let value = input.take()?;
        Ok(match kind {
            0 => Self::Add(value),
            1 => Self::Scale(value),
            2 => Self::AtLeast(value),
            3 => Self::AtMost(value),
            _ => return Err(WireError::Invalid("invalid reduction change")),
        })
    }
}
impl Wire for crate::DamageContainer {
    const MIN: usize = 53;
    fn put(&self, out: &mut Writer) {
        let v = crate::DamageValues::from(self);
        out.put(&v.original);
        out.put(&v.new_damage);
        out.put(&v.blocked);
        out.put(&v.shield_damage);
        out.put(&v.post_attack_invulnerability_ticks);
        out.put(&v.should_cause_side_effects);
        out.put(&v.reductions);
        out.put(&v.reduction_modifiers);
        out.put(&v.entered_pre);
        out.put(&v.inflicted);
        out.put(&v.health_damage);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(crate::DamageValues {
            original: input.take()?,
            new_damage: input.take()?,
            blocked: input.take()?,
            shield_damage: input.take()?,
            post_attack_invulnerability_ticks: input.take()?,
            should_cause_side_effects: input.take()?,
            reductions: input.take()?,
            reduction_modifiers: input.take()?,
            entered_pre: input.take()?,
            inflicted: input.take()?,
            health_damage: input.take()?,
        }
        .into())
    }
}

impl Wire for crate::BiomeId {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(&self.0);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(crate::__private::biome_id(input.take()?))
    }
}
impl Wire for crate::StatePredicate {
    const MIN: usize = 12;
    fn put(&self, out: &mut Writer) {
        let record = crate::StatePredicateRecord::from(self);
        match record.target {
            crate::StatePredicateTarget::Block(key) => {
                out.put(&0u32);
                out.put(&key);
            }
            crate::StatePredicateTarget::Tag(key) => {
                out.put(&1u32);
                out.put(&key);
            }
        }
        out.put(&record.constraints);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        let target = match input.take::<u32>()? {
            0 => crate::StatePredicateTarget::Block(input.take()?),
            1 => crate::StatePredicateTarget::Tag(input.take()?),
            n => return Err(WireError::Tag(n)),
        };
        // The owning game boundary validates the constraints against its metadata.
        Ok(crate::StatePredicateRecord::new(target, input.take()?).into())
    }
}
impl Wire for crate::particles::PositionSource {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        match self {
            Self::Block(pos) => {
                out.put(&0u32);
                out.put(pos)
            }
            Self::Entity { id, y_offset } => {
                out.put(&1u32);
                out.put(id);
                out.put(y_offset)
            }
        }
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(match input.take::<u32>()? {
            0 => Self::Block(input.take()?),
            1 => Self::Entity {
                id: input.take()?,
                y_offset: input.take()?,
            },
            n => return Err(WireError::Tag(n)),
        })
    }
}
impl Wire for crate::EquipmentSlotGroup {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(&(*self as u32));
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(match input.take::<u32>()? {
            0 => Self::Any,
            1 => Self::MainHand,
            2 => Self::OffHand,
            3 => Self::Hand,
            4 => Self::Feet,
            5 => Self::Legs,
            6 => Self::Chest,
            7 => Self::Head,
            8 => Self::Armor,
            9 => Self::Body,
            10 => Self::Saddle,
            n => return Err(WireError::Tag(n)),
        })
    }
}
impl Wire for crate::ItemAttributeModifier {
    const MIN: usize = 0;
    fn put(&self, out: &mut Writer) {
        out.put(&self.attribute);
        out.put(&self.modifier);
        out.put(&self.slot);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new(input.take()?, input.take()?, input.take()?))
    }
}
impl Wire for crate::ItemAttributeModifiers {
    const MIN: usize = 5;
    fn put(&self, out: &mut Writer) {
        out.put(&self.entries);
        out.put(&self.changed);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            entries: input.take()?,
            changed: input.take()?,
        })
    }
}
impl Wire for crate::ItemEnchantments {
    const MIN: usize = 5;
    fn put(&self, out: &mut Writer) {
        out.put(&self.entries);
        out.put(&self.changed);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            entries: input.take()?,
            changed: input.take()?,
        })
    }
}
impl Wire for crate::containers::ModRevision {
    const MIN: usize = 16;
    fn put(&self, out: &mut Writer) {
        out.put(&self.identity());
        out.put(&self.changes());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::from_snapshot(input.take()?, input.take()?))
    }
}
impl Wire for crate::ModBlockEntity {
    const MIN: usize = 32;
    fn retained_bytes(&self) -> Option<usize> {
        std::mem::size_of::<Self>()
            .checked_add(self.id.retained_bytes()?)?
            .checked_add(self.fields.retained_bytes()?)?
            .checked_add(self.client_update_fields.retained_bytes()?)
    }
    fn take_into(&mut self, input: &mut Reader<'_>) -> Result<(), WireError> {
        self.id.take_into(input)?;
        self.fields.nbt.take_into(input)?;
        self.fields.items.take_into(input)?;
        self.fields.data.take_into(input)?;
        self.revision = crate::containers::ModRevision::from_snapshot(input.take()?, input.take()?);
        self.client_update_fields.take_into(input)
    }
    fn put(&self, out: &mut Writer) {
        out.put(&self.id);
        out.put(&self.fields.nbt);
        out.put(&self.fields.items);
        out.put(&self.fields.data);
        out.put(&self.revision.identity());
        out.put(&self.revision.changes());
        out.put(&self.client_update_fields);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            id: input.take()?,
            fields: crate::containers::ModBlockEntityFields {
                nbt: input.take()?,
                items: input.take()?,
                data: input.take()?,
            },
            revision: crate::containers::ModRevision::from_snapshot(input.take()?, input.take()?),
            client_update_fields: input.take()?,
        })
    }
}

impl Wire for crate::ItemPlace {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        use crate::ItemPlace::*;
        match self {
            PlayerSlot { player, slot } => {
                out.put(&0u32);
                out.put(player);
                out.put(slot)
            }
            PlayerHand { player, hand } => {
                out.put(&1u32);
                out.put(player);
                out.put(hand)
            }
            PlayerCursor { menu } => {
                out.put(&2u32);
                out.put(menu)
            }
            BlockEntity {
                level,
                pos,
                side,
                slot,
            } => {
                out.put(&3u32);
                out.put(level);
                out.put(pos);
                out.put(side);
                out.put(slot)
            }
            ItemEntity { id } => {
                out.put(&4u32);
                out.put(id)
            }
            EntitySlot { entity, slot } => {
                out.put(&5u32);
                out.put(entity);
                out.put(slot)
            }
            Nested { parent, slot } => {
                out.put(&6u32);
                out.put(parent);
                out.put(slot)
            }
            MenuSlot { menu, slot } => {
                out.put(&7u32);
                out.put(menu);
                out.put(slot)
            }
            Equipment { entity, slot } => {
                out.put(&9u32);
                out.put(entity);
                out.put(slot)
            }
            Mod { provider, key } => {
                out.put(&8u32);
                out.put_str(provider);
                out.put(key)
            }
        }
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        use crate::ItemPlace::*;
        input.nested(|input| {
            Ok(match input.take::<u32>()? {
                0 => PlayerSlot {
                    player: input.take()?,
                    slot: input.take()?,
                },
                1 => PlayerHand {
                    player: input.take()?,
                    hand: input.take()?,
                },
                2 => PlayerCursor {
                    menu: input.take()?,
                },
                3 => BlockEntity {
                    level: input.take()?,
                    pos: input.take()?,
                    side: input.take()?,
                    slot: input.take()?,
                },
                4 => ItemEntity { id: input.take()? },
                5 => EntitySlot {
                    entity: input.take()?,
                    slot: input.take()?,
                },
                6 => Nested {
                    parent: input.take()?,
                    slot: input.take()?,
                },
                7 => MenuSlot {
                    menu: input.take()?,
                    slot: input.take()?,
                },
                9 => Equipment {
                    entity: input.take()?,
                    slot: input.take()?,
                },
                8 => Mod {
                    provider: crate::wire::intern(&input.take::<String>()?)?,
                    key: input.take()?,
                },
                n => return Err(WireError::Tag(n)),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn round<T: Wire + std::fmt::Debug + PartialEq>(value: T) {
        let mut out = Writer::new();
        out.put(&value);
        assert_eq!(Reader::decode::<T>(out.bytes()).unwrap(), value);
        for n in 0..out.bytes().len() {
            assert!(Reader::decode::<T>(&out.bytes()[..n]).is_err());
        }
    }

    #[test]
    fn wire_checked_values_and_garbage() {
        round(crate::ConfigPath::new("example/options.toml").unwrap());
        round(crate::SpawnerKey::new("example:spawner").unwrap());
        round(crate::CustomSpawners::vanilla());
        round(crate::Identifier::parse("example:thing").unwrap());
        round(crate::ItemPlace::Nested {
            parent: Box::new(crate::ItemPlace::Mod {
                provider: "example:provider",
                key: 3,
            }),
            slot: 4,
        });
        let mut out = Writer::new();
        out.put_str("../escape");
        assert!(Reader::decode::<crate::ConfigPath>(out.bytes()).is_err());
        out.clear();
        out.put_str("not namespaced");
        assert!(Reader::decode::<crate::SpawnerKey>(out.bytes()).is_err());
        out.clear();
        out.put(&vec![
            crate::SpawnerEntry::Builtin(crate::SpawnerKind::Cat),
            crate::SpawnerEntry::Builtin(crate::SpawnerKind::Cat),
        ]);
        assert!(Reader::decode::<crate::CustomSpawners>(out.bytes()).is_err());
        assert!(crate::wire::intern(&"x".repeat(257)).is_err());
        for len in 0..64 {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + len * 11) as u8).collect();
            let _ = Reader::decode::<crate::ItemPlace>(&bytes);
            let _ = Reader::decode::<crate::ItemStack>(&bytes);
            let _ = Reader::decode::<crate::ParticleOptions>(&bytes);
        }
    }
    #[test]
    fn wire_dimension_key_checked() {
        let mut out = Writer::new();
        out.put(&DimensionKey::OVERWORLD);
        assert_eq!(
            Reader::decode::<DimensionKey>(out.bytes()),
            Ok(DimensionKey::OVERWORLD)
        );
        assert!(Reader::decode::<DimensionKey>(&u32::MAX.to_le_bytes()).is_err());
        assert!(Reader::decode::<DimensionKey>(&[]).is_err());
    }
}

/// The whole generator, through its own constructor, which masks the state to 48 bits.
impl Wire for crate::LegacyRandom {
    const MIN: usize = 9;
    fn put(&self, out: &mut Writer) {
        out.put(&<(u64, Option<f64>)>::from(self));
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::from(input.take::<(u64, Option<f64>)>()?))
    }
}

/// The converted files of a reload, as the game hands them over: ids that parsed, with their bytes.
impl Wire for crate::reload::ReloadFiles {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(&self.0);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new(input.take()?))
    }
}
impl Wire for crate::ServerReloadContext {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(&self.files);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new(input.take()?))
    }
}
impl Wire for crate::ClientReloadContext {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(&self.files);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new(input.take()?))
    }
}

#[cfg(test)]
mod java_string_tests {
    use super::*;
    use horizon_core::nbt::JavaString;

    fn units() -> Vec<Vec<u16>> {
        vec![
            vec![],
            vec![0],
            vec![0xD800],
            vec![0xDC00, 0x0041, 0xFFFF, 0x1234],
            (0..=1000u16).map(|n| n.wrapping_mul(97)).collect(),
        ]
    }

    /// The bytes are those of a `Vec<u16>`, so the host's reader and writer need no change.
    #[test]
    fn a_key_is_the_bytes_of_its_units_both_ways() {
        for units in units() {
            let mut expected = Writer::new();
            expected.put(&units);
            let mut got = Writer::new();
            got.put(&JavaString(units.clone()));
            assert_eq!(got.bytes(), expected.bytes());
            let key: JavaString = Reader::decode(expected.bytes()).unwrap();
            assert_eq!(key.0, units);
            assert_eq!(Reader::decode::<Vec<u16>>(got.bytes()).unwrap(), units);
            for end in 0..expected.bytes().len() {
                let cut = &expected.bytes()[..end];
                assert_eq!(
                    Reader::decode::<JavaString>(cut).map(|k| k.0),
                    Reader::decode::<Vec<u16>>(cut)
                );
            }
        }
    }

    #[test]
    fn a_key_longer_than_its_bytes_or_the_limit_is_refused_before_it_grows() {
        let mut huge = Writer::new();
        huge.put(&u32::MAX);
        assert_eq!(
            Reader::decode::<JavaString>(huge.bytes()).map(|k| k.0),
            Reader::decode::<Vec<u16>>(huge.bytes())
        );
        let key = JavaString(vec![7; 10]);
        for limit in 0..30 {
            let (mut a, mut b) = (Writer::new(), Writer::new());
            a.limit(limit);
            b.limit(limit);
            a.put(&key);
            b.put(&key.0);
            assert_eq!(a.error(), b.error(), "limit {limit}");
            if a.error().is_none() {
                assert_eq!(a.bytes(), b.bytes());
            }
        }
    }
}

#[cfg(test)]
mod depth_tests {
    use super::*;

    /// Vanilla's semantic bound on a tag is 512 nested lists or compounds with any leaf inside the
    /// innermost one, mixed-type lists included: the wire's nesting budget accepts all of it.
    #[test]
    fn a_tag_at_the_semantic_depth_limit_crosses() {
        use horizon_core::nbt::{Compound, Tag};
        let mixed = Tag::List(vec![Tag::Byte(1), Tag::Short(128)]);
        let wrap_list = |inner: Tag| Tag::List(vec![inner]);
        let wrap_compound = |inner: Tag| {
            let mut compound = Compound::default();
            compound.insert("k".into(), inner);
            Tag::Compound(compound)
        };
        for wrap in [&wrap_list as &dyn Fn(Tag) -> Tag, &wrap_compound] {
            // `mixed` is itself one list: 511 wrappers make 512 nested containers.
            let mut tag = mixed.clone();
            for _ in 0..511 {
                tag = wrap(tag);
            }
            let mut out = Writer::new();
            out.put(&tag);
            let mut reader = Reader::new(out.bytes());
            assert!(reader.take::<Tag>().is_ok_and(|read| read == tag));
        }
    }
}
