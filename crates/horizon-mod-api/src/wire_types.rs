//! Checked wire decoding for opaque API values.
use crate::wire::{Reader, Wire, WireError, Writer, layout_fields};
use crate::{
    BlockEntityTypeKey, BlockKey, BoolGameRuleKey, DimensionKey, EffectKey, FluidKey, ItemKey,
    TagKey, UnresolvedFluid,
};

impl Wire for crate::OwnProvider {
    layout_fields!("handler" => String, "resource" => u64);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::OwnProvider));
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
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::Transaction));
    const MIN: usize = 0;
    fn put(&self, _: &mut Writer) {}
    fn take(_: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self(()))
    }
}
impl Wire for crate::TransactionSeal {
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::TransactionSeal));
    const MIN: usize = 0;
    fn put(&self, _: &mut Writer) {}
    fn take(_: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self(()))
    }
}
impl Wire for crate::LoadedMods {
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::LoadedMods));
    const MIN: usize = 0;
    fn put(&self, _: &mut Writer) {}
    fn take(_: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new())
    }
}
impl Wire for crate::RegistrationContext {
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::RegistrationContext));
    const MIN: usize = 0;
    fn put(&self, _: &mut Writer) {}
    fn take(_: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new())
    }
}
impl Wire for crate::PackageKind {
    layout_fields!("tag" => u32, "0:Code" => (), "1:Content" => ());
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::PackageKind));
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
    layout_fields!("tag" => u32, "0:Client" => (), "1:Both" => ());
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::Side));
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
    layout_fields!("id" => String, "version" => String, "kind" => crate::PackageKind, "side" => crate::Side, "namespaces" => Vec<String>, "dependencies" => Vec<String>);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::PackageInfo));
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
layout_fields!("slot" => $width);
            const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!($key));
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
    layout_fields!("0" => u64);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::NativeWorldEpoch));
    const MIN: usize = 8;
    fn put(&self, out: &mut Writer) {
        out.put(&self.get());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new(input.take()?))
    }
}
impl Wire for crate::NativeTickId {
    layout_fields!("0" => u64);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::NativeTickId));
    const MIN: usize = 8;
    fn put(&self, out: &mut Writer) {
        out.put(&self.get());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new(input.take()?))
    }
}
impl Wire for crate::EntityRef {
    layout_fields!("epoch" => crate::NativeWorldEpoch, "id" => u32);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::EntityRef));
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
layout_fields!("entity" => crate::EntityRef);
            const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!($ty));
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
    layout_fields!("namespace" => String, "path" => String);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::Identifier));
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
    layout_fields!("text" => String);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::LevelKey));
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put_str(&String::from(self));
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Self::try_from(input.take::<String>()?).map_err(|_| WireError::Invalid("invalid level key"))
    }
}
impl Wire for crate::ConfigPath {
    layout_fields!("text" => String);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::ConfigPath));
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put_str(self.as_str());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Self::new(&input.take::<String>()?).map_err(|_| WireError::Invalid("invalid config path"))
    }
}
impl Wire for crate::SpawnerKey {
    layout_fields!("text" => String);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::SpawnerKey));
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put_str(self.as_str());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Self::new(&input.take::<String>()?).ok_or(WireError::Invalid("invalid spawner key"))
    }
}
impl Wire for crate::CustomSpawners {
    layout_fields!("entries" => Vec<crate::SpawnerEntry>);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::CustomSpawners));
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
    layout_fields!("prefix" => String, "extension" => String);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::FileToIdConverter));
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
    layout_fields!("id" => crate::Identifier, "message" => String);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::ReloadResourceError));
    const MIN: usize = 12;
    fn put(&self, out: &mut Writer) {
        out.put(&<(crate::Identifier, String)>::from(self));
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::from(input.take::<(crate::Identifier, String)>()?))
    }
}
impl Wire for crate::BlockState {
    layout_fields!("block" => crate::BlockKey, "ordinal" => u16);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::BlockState));
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
    layout_fields!("first" => crate::BlockState, "len" => u32);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::BlockStateRange));
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
    layout_fields!("units" => Vec<u16>);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(horizon_core::nbt::JavaString));
    const MIN: usize = <Vec<u16> as Wire>::MIN;
    fn put(&self, out: &mut Writer) {
        out.put_u16s(&self.0);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        input.nested(|input| input.take_u16s().map(Self))
    }
}

impl Wire for crate::Components {
    layout_fields!("entries" => horizon_core::nbt::Compound);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::Components));
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(self.as_nbt());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::from(input.take::<horizon_core::nbt::Compound>()?))
    }
}
impl Wire for crate::ItemStack {
    layout_fields!("item" => crate::ItemKey, "count" => u32, "damage" => Option<u32>, "components" => Option<crate::Components>);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::ItemStack));
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
    layout_fields!("tag" => u32, "0:Builtin" => crate::SpawnerKind, "1:Mod" => crate::SpawnerKey);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::SpawnerEntry));
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
    layout_fields!("epoch" => crate::NativeWorldEpoch, "dimension" => crate::DimensionKey);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::LevelRef));
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
    layout_fields!("level" => crate::LevelRef, "pos" => crate::ChunkPos);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::ChunkRef));
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
    layout_fields!("level" => crate::LevelRef, "id" => u32);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::RaidRef));
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
    layout_fields!("entity" => crate::EntityRef, "uuid" => u128);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::PlayerRef));
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
    layout_fields!("dimension" => crate::Identifier, "pos" => crate::BlockPos);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::GlobalPos));
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
    layout_fields!("0" => u64);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::SoundRef));
    const MIN: usize = 8;
    fn put(&self, out: &mut Writer) {
        out.put(&self.id);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self { id: input.take()? })
    }
}
impl Wire for crate::BossBarRef {
    layout_fields!("epoch" => crate::NativeWorldEpoch, "uuid" => u128);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::BossBarRef));
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
    layout_fields!("player" => crate::PlayerRef, "window" => i32, "generation" => u64);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::MenuRef));
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
    layout_fields!("level" => crate::LevelRef, "position" => crate::BlockPos, "generation" => u64);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::BlockEntityRef));
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
    layout_fields!("epoch" => crate::NativeWorldEpoch, "lend" => u64);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::UnspawnedMobRef));
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
    layout_fields!("generation" => u64, "slot" => u32);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::ModelRef));
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
    layout_fields!("generation" => u64, "slot" => u32);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::InstanceGroupRef));
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
    layout_fields!("0" => u32);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::TicketController));
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(&self.slot());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Self::from_slot(input.take()?).ok_or(WireError::Invalid("unknown ticket controller"))
    }
}

impl Wire for crate::SpawnerKind {
    layout_fields!("tag" => u32, "0:Phantom" => (), "1:Patrol" => (), "2:Cat" => (), "3:VillageSiege" => (), "4:WanderingTrader" => ());
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::SpawnerKind));
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
    layout_fields!("0" => u32);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::NativeBlockUpdateFlags));
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(&self.bits());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self(input.take()?))
    }
}
impl Wire for crate::FeatureFlags {
    layout_fields!("flags" => Vec<String>);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::FeatureFlags));
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(&self.iter().map(str::to_owned).collect::<Vec<_>>());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new(input.take::<Vec<String>>()?.into()))
    }
}
impl Wire for crate::NativeMobGoalEntry {
    layout_fields!("priority" => i32, "id" => String, "running" => bool);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::NativeMobGoalEntry));
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
    layout_fields!("tag" => u32, "0:VanillaType.kind" => crate::BlockEntityTypeKey, "1:Duplicate.kind" => crate::BlockEntityTypeKey, "1:Duplicate.part" => String);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::BlockEntityPartError));
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
    layout_fields!("stack" => crate::ItemStack);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::ItemResource));
    const MIN: usize = crate::ItemStack::MIN;
    fn put(&self, out: &mut Writer) {
        out.put(&self.0);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::of(&input.take()?))
    }
}
impl Wire for crate::FluidResource {
    layout_fields!("stack" => crate::FluidStack);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::FluidResource));
    const MIN: usize = crate::FluidStack::MIN;
    fn put(&self, out: &mut Writer) {
        out.put(&self.0);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::of(&input.take()?))
    }
}
impl Wire for crate::Corners {
    layout_fields!("width" => f32, "height" => f32, "used" => [f32;4]);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::Corners));
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
    layout_fields!("ty" => crate::ParticleKey, "block" => Option<(String,u32)>, "color" => [f32;4], "scale" => f32, "to_color" => [f32;3], "trail" => Option<([f64;3],i32)>, "item" => Option<Vec<String>>, "geyser" => Option<(i32,f32)>, "vibration" => Option<(crate::particles::PositionSource,[f64;3],i32)>, "legacy_entity_effect" => Option<bool>, "legacy_trail_lifetime" => bool);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::ParticleOptions));
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
    layout_fields!("tag" => u8, "0:Invulnerability" => (), "1:Armor" => (), "2:Enchantments" => (), "3:MobEffects" => (), "4:Absorption" => (), "5:InnateResistance" => ());
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::Reduction));
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
    layout_fields!("tag" => u8, "0:Add" => f32, "1:Scale" => f32, "2:AtLeast" => f32, "3:AtMost" => f32);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::ReductionChange));
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
    layout_fields!("snapshot" => crate::DamageValues);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::DamageContainer));
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
    layout_fields!("0" => u32);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::BiomeId));
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(&self.0);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(crate::__private::biome_id(input.take()?))
    }
}
impl Wire for crate::ModColorResolver {
    layout_fields!("0" => u32);
    const LAYOUT_SEED: u64 = crate::wire::layout_name("ModColorResolver");
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(&self.0);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(crate::__private::mod_color_resolver(input.take()?))
    }
}
impl Wire for crate::StatePredicate {
    layout_fields!("target" => crate::StatePredicateTarget, "constraints" => Vec<crate::StateConstraint>);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::StatePredicate));
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
    layout_fields!("tag" => u32, "0:Block" => crate::BlockPos, "1:Entity.id" => u32, "1:Entity.y_offset" => f32);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::particles::PositionSource));
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
    layout_fields!("tag" => u32, "0:Any" => (), "1:MainHand" => (), "2:OffHand" => (), "3:Hand" => (), "4:Feet" => (), "5:Legs" => (), "6:Chest" => (), "7:Head" => (), "8:Armor" => (), "9:Body" => (), "10:Saddle" => ());
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::EquipmentSlotGroup));
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
    layout_fields!("attribute" => crate::Identifier, "modifier" => crate::AttributeModifier, "slot" => crate::EquipmentSlotGroup);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::ItemAttributeModifier));
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
    layout_fields!("entries" => Vec<crate::ItemAttributeModifier>, "changed" => bool);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::ItemAttributeModifiers));
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
    layout_fields!("entries" => Vec<(crate::Identifier,u32)>, "changed" => bool);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::ItemEnchantments));
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
    layout_fields!("born" => u64, "changes" => u64);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::containers::ModRevision));
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
    layout_fields!("snapshot" => crate::containers::ModBlockEntitySnapshot);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::ModBlockEntity));
    const MIN: usize = 32;
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
    layout_fields!("tag" => u32, "0:PlayerSlot.player" => u128, "0:PlayerSlot.slot" => i32, "1:PlayerHand.player" => u128, "1:PlayerHand.hand" => crate::Hand, "2:PlayerCursor.menu" => crate::MenuRef, "3:BlockEntity.level" => crate::LevelRef, "3:BlockEntity.pos" => [i32;3], "3:BlockEntity.side" => Option<crate::Direction>, "3:BlockEntity.slot" => usize, "4:ItemEntity.id" => u32, "5:EntitySlot.entity" => u32, "5:EntitySlot.slot" => usize, "6:Nested.parent" => Box<crate::ItemPlace>, "6:Nested.slot" => usize, "7:MenuSlot.menu" => crate::MenuRef, "7:MenuSlot.slot" => usize, "8:Mod.provider" => String, "8:Mod.key" => u64, "9:Equipment.entity" => u32, "9:Equipment.slot" => crate::EquipmentSlot);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::ItemPlace));
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
    layout_fields!("seed" => u64, "gaussian" => Option<f64>);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::LegacyRandom));
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
    layout_fields!("files" => std::collections::BTreeMap<crate::Identifier,Vec<u8>>);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::reload::ReloadFiles));
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(&self.0);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new(input.take()?))
    }
}
impl Wire for crate::ServerReloadContext {
    layout_fields!("files" => crate::reload::ReloadFiles);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::ServerReloadContext));
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(&self.files);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self::new(input.take()?))
    }
}
impl Wire for crate::ClientReloadContext {
    layout_fields!("files" => crate::reload::ReloadFiles);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::ClientReloadContext));
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

// Re-exported types cannot carry a local derive. Keep their encoding explicit here.
impl crate::wire::Wire for crate::Tag {
    layout_fields!("tag" => u32, "0:End" => (), "1:Byte" => i8, "2:Short" => i16, "3:Int" => i32, "4:Long" => i64, "5:Float" => f32, "6:Double" => f64, "7:ByteArray" => Vec<u8>, "8:String" => horizon_core::nbt::JavaString, "9:List" => Vec<crate::Tag>, "10:Compound" => horizon_core::nbt::Compound, "11:IntArray" => Vec<i32>, "12:LongArray" => Vec<i64>);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::Tag));
    const MIN: usize = 4;
    fn put(&self, out: &mut crate::wire::Writer) {
        match self {
            Self::End => out.put(&0u32),
            Self::Byte(f0) => {
                out.put(&1u32);
                out.put(f0);
            }
            Self::Short(f0) => {
                out.put(&2u32);
                out.put(f0);
            }
            Self::Int(f0) => {
                out.put(&3u32);
                out.put(f0);
            }
            Self::Long(f0) => {
                out.put(&4u32);
                out.put(f0);
            }
            Self::Float(f0) => {
                out.put(&5u32);
                out.put(f0);
            }
            Self::Double(f0) => {
                out.put(&6u32);
                out.put(f0);
            }
            Self::ByteArray(f0) => {
                out.put(&7u32);
                out.put(f0);
            }
            Self::String(f0) => {
                out.put(&8u32);
                out.put(f0);
            }
            Self::List(f0) => {
                out.put(&9u32);
                out.put(f0);
            }
            Self::Compound(f0) => {
                out.put(&10u32);
                out.put(f0);
            }
            Self::IntArray(f0) => {
                out.put(&11u32);
                out.put(f0);
            }
            Self::LongArray(f0) => {
                out.put(&12u32);
                out.put(f0);
            }
        }
    }
    fn put_owned(self, out: &mut crate::wire::Writer) {
        match self {
            Self::End => out.put_owned(0u32),
            Self::Byte(f0) => {
                out.put_owned(1u32);
                out.put_owned(f0);
            }
            Self::Short(f0) => {
                out.put_owned(2u32);
                out.put_owned(f0);
            }
            Self::Int(f0) => {
                out.put_owned(3u32);
                out.put_owned(f0);
            }
            Self::Long(f0) => {
                out.put_owned(4u32);
                out.put_owned(f0);
            }
            Self::Float(f0) => {
                out.put_owned(5u32);
                out.put_owned(f0);
            }
            Self::Double(f0) => {
                out.put_owned(6u32);
                out.put_owned(f0);
            }
            Self::ByteArray(f0) => {
                out.put_owned(7u32);
                out.put_owned(f0);
            }
            Self::String(f0) => {
                out.put_owned(8u32);
                out.put_owned(f0);
            }
            Self::List(f0) => {
                out.put_owned(9u32);
                out.put_owned(f0);
            }
            Self::Compound(f0) => {
                out.put_owned(10u32);
                out.put_owned(f0);
            }
            Self::IntArray(f0) => {
                out.put_owned(11u32);
                out.put_owned(f0);
            }
            Self::LongArray(f0) => {
                out.put_owned(12u32);
                out.put_owned(f0);
            }
        }
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        input.nested(|input| {
            Ok(match input.take::<u32>()? {
                0 => Self::End,
                1 => Self::Byte(input.take()?),
                2 => Self::Short(input.take()?),
                3 => Self::Int(input.take()?),
                4 => Self::Long(input.take()?),
                5 => Self::Float(input.take()?),
                6 => Self::Double(input.take()?),
                7 => Self::ByteArray(input.take()?),
                8 => Self::String(input.take()?),
                9 => Self::List(input.take()?),
                10 => Self::Compound(input.take()?),
                11 => Self::IntArray(input.take()?),
                12 => Self::LongArray(input.take()?),
                tag => return Err(crate::wire::WireError::Tag(tag)),
            })
        })
    }
}
impl crate::wire::Wire for serde_json::Value {
    layout_fields!("json" => String);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(serde_json::Value));
    const MIN: usize = 4;
    fn put(&self, out: &mut crate::wire::Writer) {
        out.put_str(&::serde_json::to_string(self).unwrap_or_default());
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        let text: ::std::string::String = input.take()?;
        ::serde_json::from_str(&text)
            .map_err(|_| crate::wire::WireError::Invalid("not valid JSON for its type"))
    }
}
impl crate::wire::Wire for crate::Direction {
    layout_fields!("tag" => u32, "0:Down" => (), "1:Up" => (), "2:North" => (), "3:South" => (), "4:West" => (), "5:East" => ());
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::Direction));
    const MIN: usize = 4;
    fn put(&self, out: &mut crate::wire::Writer) {
        out.put(&match self {
            Self::Down => 0u32,
            Self::Up => 1u32,
            Self::North => 2u32,
            Self::South => 3u32,
            Self::West => 4u32,
            Self::East => 5u32,
        });
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        Ok(match input.take::<u32>()? {
            0 => Self::Down,
            1 => Self::Up,
            2 => Self::North,
            3 => Self::South,
            4 => Self::West,
            5 => Self::East,
            tag => return Err(crate::wire::WireError::Tag(tag)),
        })
    }
}
impl crate::wire::Wire for crate::BlockPos {
    layout_fields!("x" => i32, "y" => i32, "z" => i32);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::BlockPos));
    const MIN: usize = <i32 as crate::wire::Wire>::MIN
        + <i32 as crate::wire::Wire>::MIN
        + <i32 as crate::wire::Wire>::MIN;
    fn put(&self, out: &mut crate::wire::Writer) {
        out.put(&self.x);
        out.put(&self.y);
        out.put(&self.z);
    }
    fn put_owned(self, out: &mut crate::wire::Writer) {
        out.put_owned(self.x);
        out.put_owned(self.y);
        out.put_owned(self.z);
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        input.nested(|input| {
            Ok(Self {
                x: input.take()?,
                y: input.take()?,
                z: input.take()?,
            })
        })
    }
}
impl crate::wire::Wire for crate::EquipmentSlot {
    layout_fields!("tag" => u32, "0:MainHand" => (), "1:OffHand" => (), "2:Feet" => (), "3:Legs" => (), "4:Chest" => (), "5:Head" => (), "6:Body" => (), "7:Saddle" => ());
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::EquipmentSlot));
    const MIN: usize = 4;
    fn put(&self, out: &mut crate::wire::Writer) {
        out.put(&match self {
            Self::MainHand => 0u32,
            Self::OffHand => 1u32,
            Self::Feet => 2u32,
            Self::Legs => 3u32,
            Self::Chest => 4u32,
            Self::Head => 5u32,
            Self::Body => 6u32,
            Self::Saddle => 7u32,
        });
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        Ok(match input.take::<u32>()? {
            0 => Self::MainHand,
            1 => Self::OffHand,
            2 => Self::Feet,
            3 => Self::Legs,
            4 => Self::Chest,
            5 => Self::Head,
            6 => Self::Body,
            7 => Self::Saddle,
            tag => return Err(crate::wire::WireError::Tag(tag)),
        })
    }
}
impl crate::wire::Wire for crate::GameMode {
    layout_fields!("tag" => u32, "0:Survival" => (), "1:Creative" => (), "2:Adventure" => (), "3:Spectator" => ());
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::GameMode));
    const MIN: usize = 4;
    fn put(&self, out: &mut crate::wire::Writer) {
        out.put(&match self {
            Self::Survival => 0u32,
            Self::Creative => 1u32,
            Self::Adventure => 2u32,
            Self::Spectator => 3u32,
        });
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        Ok(match input.take::<u32>()? {
            0 => Self::Survival,
            1 => Self::Creative,
            2 => Self::Adventure,
            3 => Self::Spectator,
            tag => return Err(crate::wire::WireError::Tag(tag)),
        })
    }
}
impl crate::wire::Wire for crate::NeighbourUpdate {
    layout_fields!("tag" => u32, "0:OutputSignal" => (), "1:Neighbours" => (), "2:NeighboursExcept" => crate::Direction);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::NeighbourUpdate));
    const MIN: usize = 4;
    fn put(&self, out: &mut crate::wire::Writer) {
        match self {
            Self::OutputSignal => out.put(&0u32),
            Self::Neighbours => out.put(&1u32),
            Self::NeighboursExcept(f0) => {
                out.put(&2u32);
                out.put(f0);
            }
        }
    }
    fn put_owned(self, out: &mut crate::wire::Writer) {
        match self {
            Self::OutputSignal => out.put_owned(0u32),
            Self::Neighbours => out.put_owned(1u32),
            Self::NeighboursExcept(f0) => {
                out.put_owned(2u32);
                out.put_owned(f0);
            }
        }
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        input.nested(|input| {
            Ok(match input.take::<u32>()? {
                0 => Self::OutputSignal,
                1 => Self::Neighbours,
                2 => Self::NeighboursExcept(input.take()?),
                tag => return Err(crate::wire::WireError::Tag(tag)),
            })
        })
    }
}
impl crate::wire::Wire for crate::ChunkPos {
    layout_fields!("x" => i32, "z" => i32);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::ChunkPos));
    const MIN: usize = <i32 as crate::wire::Wire>::MIN + <i32 as crate::wire::Wire>::MIN;
    fn put(&self, out: &mut crate::wire::Writer) {
        out.put(&self.x);
        out.put(&self.z);
    }
    fn put_owned(self, out: &mut crate::wire::Writer) {
        out.put_owned(self.x);
        out.put_owned(self.z);
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        input.nested(|input| {
            Ok(Self {
                x: input.take()?,
                z: input.take()?,
            })
        })
    }
}
impl crate::wire::Wire for crate::BoundingBox {
    layout_fields!("min" => [i32;3], "max" => [i32;3]);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::BoundingBox));
    const MIN: usize = <[i32; 3] as crate::wire::Wire>::MIN + <[i32; 3] as crate::wire::Wire>::MIN;
    fn put(&self, out: &mut crate::wire::Writer) {
        out.put(&self.min);
        out.put(&self.max);
    }
    fn put_owned(self, out: &mut crate::wire::Writer) {
        out.put_owned(self.min);
        out.put_owned(self.max);
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        input.nested(|input| {
            Ok(Self {
                min: input.take()?,
                max: input.take()?,
            })
        })
    }
}
impl crate::wire::Wire for crate::LevelDimension {
    layout_fields!("tag" => u32, "0:Overworld" => (), "1:Nether" => (), "2:End" => (), "3:Custom" => crate::LevelKey);
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::LevelDimension));
    const MIN: usize = 4;
    fn put(&self, out: &mut crate::wire::Writer) {
        match self {
            Self::Overworld => out.put(&0u32),
            Self::Nether => out.put(&1u32),
            Self::End => out.put(&2u32),
            Self::Custom(f0) => {
                out.put(&3u32);
                out.put(f0);
            }
        }
    }
    fn put_owned(self, out: &mut crate::wire::Writer) {
        match self {
            Self::Overworld => out.put_owned(0u32),
            Self::Nether => out.put_owned(1u32),
            Self::End => out.put_owned(2u32),
            Self::Custom(f0) => {
                out.put_owned(3u32);
                out.put_owned(f0);
            }
        }
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        input.nested(|input| {
            Ok(match input.take::<u32>()? {
                0 => Self::Overworld,
                1 => Self::Nether,
                2 => Self::End,
                3 => Self::Custom(input.take()?),
                tag => return Err(crate::wire::WireError::Tag(tag)),
            })
        })
    }
}
impl crate::wire::Wire for serde_json::Map<String, serde_json::Value> {
    layout_fields!("json" => String);
    const LAYOUT_SEED: u64 =
        crate::wire::layout_seed(stringify!(serde_json::Map<String, serde_json::Value>));
    const MIN: usize = 4;
    fn put(&self, out: &mut crate::wire::Writer) {
        out.put_str(&::serde_json::to_string(self).unwrap_or_default());
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        let text: ::std::string::String = input.take()?;
        ::serde_json::from_str(&text)
            .map_err(|_| crate::wire::WireError::Invalid("not valid JSON for its type"))
    }
}
impl crate::wire::Wire for crate::ArmPose {
    layout_fields!("tag" => u32, "0:Empty" => (), "1:Item" => (), "2:Block" => (), "3:Bow" => (), "4:CrossbowHold" => (), "5:CrossbowCharge" => (), "6:ThrowTrident" => (), "7:TootHorn" => (), "8:Spyglass" => (), "9:Spear" => (), "10:Brush" => ());
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::ArmPose));
    const MIN: usize = 4;
    fn put(&self, out: &mut crate::wire::Writer) {
        out.put(&match self {
            Self::Empty => 0u32,
            Self::Item => 1u32,
            Self::Block => 2u32,
            Self::Bow => 3u32,
            Self::CrossbowHold => 4u32,
            Self::CrossbowCharge => 5u32,
            Self::ThrowTrident => 6u32,
            Self::TootHorn => 7u32,
            Self::Spyglass => 8u32,
            Self::Spear => 9u32,
            Self::Brush => 10u32,
        });
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        Ok(match input.take::<u32>()? {
            0 => Self::Empty,
            1 => Self::Item,
            2 => Self::Block,
            3 => Self::Bow,
            4 => Self::CrossbowHold,
            5 => Self::CrossbowCharge,
            6 => Self::ThrowTrident,
            7 => Self::TootHorn,
            8 => Self::Spyglass,
            9 => Self::Spear,
            10 => Self::Brush,
            tag => return Err(crate::wire::WireError::Tag(tag)),
        })
    }
}
impl crate::wire::Wire for crate::UseAnimation {
    layout_fields!("tag" => u32, "0:None" => (), "1:Eat" => (), "2:Drink" => (), "3:Block" => (), "4:Bow" => (), "5:Trident" => (), "6:Crossbow" => (), "7:Spyglass" => (), "8:TootHorn" => (), "9:Brush" => (), "10:Bundle" => (), "11:Spear" => ());
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::UseAnimation));
    const MIN: usize = 4;
    fn put(&self, out: &mut crate::wire::Writer) {
        out.put(&match self {
            Self::None => 0u32,
            Self::Eat => 1u32,
            Self::Drink => 2u32,
            Self::Block => 3u32,
            Self::Bow => 4u32,
            Self::Trident => 5u32,
            Self::Crossbow => 6u32,
            Self::Spyglass => 7u32,
            Self::TootHorn => 8u32,
            Self::Brush => 9u32,
            Self::Bundle => 10u32,
            Self::Spear => 11u32,
        });
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        Ok(match input.take::<u32>()? {
            0 => Self::None,
            1 => Self::Eat,
            2 => Self::Drink,
            3 => Self::Block,
            4 => Self::Bow,
            5 => Self::Trident,
            6 => Self::Crossbow,
            7 => Self::Spyglass,
            8 => Self::TootHorn,
            9 => Self::Brush,
            10 => Self::Bundle,
            11 => Self::Spear,
            tag => return Err(crate::wire::WireError::Tag(tag)),
        })
    }
}
impl crate::wire::Wire for crate::ConnectionKind {
    layout_fields!("tag" => u32, "0:Fence" => (), "1:Wall" => (), "2:Bars" => ());
    const LAYOUT_SEED: u64 = crate::wire::layout_seed(stringify!(crate::ConnectionKind));
    const MIN: usize = 4;
    fn put(&self, out: &mut crate::wire::Writer) {
        out.put(&match self {
            Self::Fence => 0u32,
            Self::Wall => 1u32,
            Self::Bars => 2u32,
            _ => {
                out.fail(crate::wire::WireError::Invalid("unknown ConnectionKind"));
                return;
            }
        });
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        Ok(match input.take::<u32>()? {
            0 => Self::Fence,
            1 => Self::Wall,
            2 => Self::Bars,
            tag => return Err(crate::wire::WireError::Tag(tag)),
        })
    }
}

#[allow(unreachable_patterns, unused_variables)]
impl crate::wire::Wire for crate::AttachmentRowError {
    layout_fields!("tag" => u32, "0:Value" => crate::AttachmentError, "1:StaleHolder" => ());
    const LAYOUT_SEED: u64 = crate::wire::layout_name("AttachmentRowError");
    const MIN: usize = 4;
    fn put(&self, out: &mut crate::wire::Writer) {
        match self {
            Self::Value(f0) => {
                out.put(&0u32);
                out.put(f0);
            }
            Self::StaleHolder => out.put(&1u32),
            _ => {
                out.fail(crate::wire::WireError::Invalid(
                    "unknown AttachmentRowError",
                ));
            }
        }
    }
    fn put_owned(self, out: &mut crate::wire::Writer) {
        match self {
            Self::Value(f0) => {
                out.put_owned(0u32);
                out.put_owned(f0);
            }
            Self::StaleHolder => out.put_owned(1u32),
            _ => {
                out.fail(crate::wire::WireError::Invalid(
                    "unknown AttachmentRowError",
                ));
            }
        }
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        input.nested(|input| {
            Ok(match input.take::<u32>()? {
                0 => Self::Value(input.take()?),
                1 => Self::StaleHolder,
                tag => return Err(crate::wire::WireError::Tag(tag)),
            })
        })
    }
}
#[allow(unreachable_patterns, unused_variables)]
impl crate::wire::Wire for crate::AttachmentError {
    layout_fields!("tag" => u32, "0:MissingHolder" => (), "1:TypeMismatch" => (), "2:Decode" => (), "3:Encode" => (), "4:InvalidNamespace" => (), "5:InvalidCopyPolicy" => (), "6:InvalidSyncPolicy" => ());
    const LAYOUT_SEED: u64 = crate::wire::layout_name("AttachmentError");
    const MIN: usize = 4;
    fn put(&self, out: &mut crate::wire::Writer) {
        out.put(&match self {
            Self::MissingHolder => 0u32,
            Self::TypeMismatch => 1u32,
            Self::Decode => 2u32,
            Self::Encode => 3u32,
            Self::InvalidNamespace => 4u32,
            Self::InvalidCopyPolicy => 5u32,
            Self::InvalidSyncPolicy => 6u32,
            _ => {
                out.fail(crate::wire::WireError::Invalid("unknown AttachmentError"));
                return;
            }
        });
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        Ok(match input.take::<u32>()? {
            0 => Self::MissingHolder,
            1 => Self::TypeMismatch,
            2 => Self::Decode,
            3 => Self::Encode,
            4 => Self::InvalidNamespace,
            5 => Self::InvalidCopyPolicy,
            6 => Self::InvalidSyncPolicy,
            tag => return Err(crate::wire::WireError::Tag(tag)),
        })
    }
}
#[allow(unreachable_patterns, unused_variables)]
impl crate::wire::Wire for crate::NativeTicketOwner {
    layout_fields!("tag" => u32, "0:Block" => [i32;3], "1:Entity" => u128);
    const LAYOUT_SEED: u64 = crate::wire::layout_name("NativeTicketOwner");
    const MIN: usize = 4;
    fn put(&self, out: &mut crate::wire::Writer) {
        match self {
            Self::Block(f0) => {
                out.put(&0u32);
                out.put(f0);
            }
            Self::Entity(f0) => {
                out.put(&1u32);
                out.put(f0);
            }
        }
    }
    fn put_owned(self, out: &mut crate::wire::Writer) {
        match self {
            Self::Block(f0) => {
                out.put_owned(0u32);
                out.put_owned(f0);
            }
            Self::Entity(f0) => {
                out.put_owned(1u32);
                out.put_owned(f0);
            }
        }
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        input.nested(|input| {
            Ok(match input.take::<u32>()? {
                0 => Self::Block(input.take()?),
                1 => Self::Entity(input.take()?),
                tag => return Err(crate::wire::WireError::Tag(tag)),
            })
        })
    }
}
#[allow(unreachable_patterns, unused_variables)]
impl crate::wire::Wire for crate::Holder {
    layout_fields!("tag" => u32, "0:Entity" => u32, "1:Player" => u128, "2:BlockEntity" => [i32;3], "3:Chunk" => [i32;2], "4:Level" => ());
    const LAYOUT_SEED: u64 = crate::wire::layout_name("Holder");
    const MIN: usize = 4;
    fn put(&self, out: &mut crate::wire::Writer) {
        match self {
            Self::Entity(f0) => {
                out.put(&0u32);
                out.put(f0);
            }
            Self::Player(f0) => {
                out.put(&1u32);
                out.put(f0);
            }
            Self::BlockEntity(f0) => {
                out.put(&2u32);
                out.put(f0);
            }
            Self::Chunk(f0) => {
                out.put(&3u32);
                out.put(f0);
            }
            Self::Level => out.put(&4u32),
            _ => {
                out.fail(crate::wire::WireError::Invalid("unknown Holder"));
            }
        }
    }
    fn put_owned(self, out: &mut crate::wire::Writer) {
        match self {
            Self::Entity(f0) => {
                out.put_owned(0u32);
                out.put_owned(f0);
            }
            Self::Player(f0) => {
                out.put_owned(1u32);
                out.put_owned(f0);
            }
            Self::BlockEntity(f0) => {
                out.put_owned(2u32);
                out.put_owned(f0);
            }
            Self::Chunk(f0) => {
                out.put_owned(3u32);
                out.put_owned(f0);
            }
            Self::Level => out.put_owned(4u32),
            _ => {
                out.fail(crate::wire::WireError::Invalid("unknown Holder"));
            }
        }
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        input.nested(|input| {
            Ok(match input.take::<u32>()? {
                0 => Self::Entity(input.take()?),
                1 => Self::Player(input.take()?),
                2 => Self::BlockEntity(input.take()?),
                3 => Self::Chunk(input.take()?),
                4 => Self::Level,
                tag => return Err(crate::wire::WireError::Tag(tag)),
            })
        })
    }
}
#[allow(unreachable_patterns, unused_variables)]
impl crate::wire::Wire for crate::AttachmentForm {
    layout_fields!("encoded" => Option<horizon_core::nbt::Compound>, "policy" => crate::AttachmentPolicy, "write" => crate::AttachmentWrite);
    const LAYOUT_SEED: u64 = crate::wire::layout_name("AttachmentForm");
    const MIN: usize = <::std::option::Option<
        std::collections::BTreeMap<horizon_core::nbt::JavaString, crate::Tag>,
    > as crate::wire::Wire>::MIN
        + <crate::AttachmentPolicy as crate::wire::Wire>::MIN
        + <crate::AttachmentWrite as crate::wire::Wire>::MIN;
    fn put(&self, out: &mut crate::wire::Writer) {
        out.put(&self.encoded);
        out.put(&self.policy);
        out.put(&self.write);
    }
    fn put_owned(self, out: &mut crate::wire::Writer) {
        out.put_owned(self.encoded);
        out.put_owned(self.policy);
        out.put_owned(self.write);
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        input.nested(|input| {
            Ok(Self {
                encoded: input.take()?,
                policy: input.take()?,
                write: input.take()?,
            })
        })
    }
}
#[allow(unreachable_patterns, unused_variables)]
impl crate::wire::Wire for crate::AttachmentPolicy {
    layout_fields!("copy_on_death" => bool, "sync" => bool);
    const LAYOUT_SEED: u64 = crate::wire::layout_name("AttachmentPolicy");
    const MIN: usize = <bool as crate::wire::Wire>::MIN + <bool as crate::wire::Wire>::MIN;
    fn put(&self, out: &mut crate::wire::Writer) {
        out.put(&self.copy_on_death);
        out.put(&self.sync);
    }
    fn put_owned(self, out: &mut crate::wire::Writer) {
        out.put_owned(self.copy_on_death);
        out.put_owned(self.sync);
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        input.nested(|input| {
            Ok(Self {
                copy_on_death: input.take()?,
                sync: input.take()?,
            })
        })
    }
}
#[allow(unreachable_patterns, unused_variables)]
impl crate::wire::Wire for crate::AttachmentWrite {
    layout_fields!("tag" => u32, "0:Decoded" => (), "1:Default" => (), "2:Set" => ());
    const LAYOUT_SEED: u64 = crate::wire::layout_name("AttachmentWrite");
    const MIN: usize = 4;
    fn put(&self, out: &mut crate::wire::Writer) {
        out.put(&match self {
            Self::Decoded => 0u32,
            Self::Default => 1u32,
            Self::Set => 2u32,
            _ => {
                out.fail(crate::wire::WireError::Invalid("unknown AttachmentWrite"));
                return;
            }
        });
    }
    fn take(
        input: &mut crate::wire::Reader<'_>,
    ) -> ::std::result::Result<Self, crate::wire::WireError> {
        Ok(match input.take::<u32>()? {
            0 => Self::Decoded,
            1 => Self::Default,
            2 => Self::Set,
            tag => return Err(crate::wire::WireError::Tag(tag)),
        })
    }
}

#[cfg(test)]
#[path = "wire_layout_tests.rs"]
mod layout_tests;
