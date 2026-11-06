//! Typed native-mod data attached to server-owned holders.
#[doc(hidden)]
pub mod index;

use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::{Arc, OnceLock},
};

use crate::nbt::{Compound, JavaString, Tag};

/// An attachment operation could not be completed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttachmentError {
    /// The holder does not exist or is unavailable on this side.
    MissingHolder,
    /// The key was used with a different Rust type.
    TypeMismatch,
    /// The saved value could not be decoded.
    Decode,
    /// The value could not be encoded.
    Encode,
    /// A namespace in the saved compound is not a compound.
    InvalidNamespace,
    /// Copy-on-death requires a serializer.
    InvalidCopyPolicy,
    /// Syncing to clients requires a serializer.
    InvalidSyncPolicy,
}

/// NBT key reserved for server-owned attachment maps.
pub const ATTACHMENTS_KEY: &str = "horizon:attachments";

/// Read the reserved compound without replacing an invalid saved value.
pub fn attachment_compound(tag: Option<&Tag>) -> Result<Option<&Compound>, AttachmentError> {
    match tag {
        None => Ok(None),
        Some(Tag::Compound(compound)) => Ok(Some(compound)),
        Some(_) => Err(AttachmentError::InvalidNamespace),
    }
}

/// A server holder in the host's current dimension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Holder {
    Entity(u32),
    Player(u128),
    BlockEntity([i32; 3]),
    Chunk([i32; 2]),
    Level,
}

/// The game's access to each holder's map: every [`AttachmentStore`] answers through it.
pub trait AttachmentHost {
    fn attachments(&mut self, holder: Holder) -> Result<&mut Attachments, AttachmentError>;
}

/// A value of a mod's own type, as the holder keeps it: shared, so a copy of the holder's map
/// copies no value.
pub type AttachmentValue = Arc<dyn Any + Send + Sync>;

/// How a policy-checked type declared its value: what saving, respawning and syncing do with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttachmentPolicy {
    pub copy_on_death: bool,
    pub sync: bool,
}

/// What comes with a stored value: its saved form (`None` for a type with no codec), its type's
/// policy, and why it is stored.
#[derive(Clone, Debug, PartialEq)]
pub struct AttachmentForm {
    pub encoded: Option<Compound>,
    pub policy: AttachmentPolicy,
    pub write: AttachmentWrite,
}

/// Why a value is stored, which decides what else changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttachmentWrite {
    /// A saved value just decoded: nothing else changes.
    Decoded,
    /// The type's default, inserted by a read: clients hear of it if the type syncs.
    Default,
    /// A value a mod set: clients hear of it if the type syncs, and the saved form it replaces
    /// is dropped.
    Set,
}

/// The current server's holders' values, erased to what crosses any boundary: the typed
/// [`AttachmentType`] methods are built on it, for a native mod and a wasm mod alike.
pub trait AttachmentStore {
    /// Whether `holder` keeps a value under `namespace:name`, saved or decoded.
    fn holds(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
    ) -> Result<bool, AttachmentError>;
    /// The value under `namespace:name` if it has been decoded.
    fn decoded(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
    ) -> Result<Option<AttachmentValue>, AttachmentError>;
    /// The saved form under `namespace:name` if it hasn't been decoded yet.
    fn saved(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
    ) -> Result<Option<Compound>, AttachmentError>;
    /// Keep `value` under `namespace:name`, with what comes with it.
    fn store(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
        value: AttachmentValue,
        form: AttachmentForm,
    ) -> Result<(), AttachmentError>;
    /// Drop the value under `namespace:name`, saved or decoded; clients hear of it if `sync`.
    fn discard(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
        sync: bool,
    ) -> Result<(), AttachmentError>;
    /// Send the value under `namespace:name` to the clients that see the holder again.
    fn queue_sync(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
    ) -> Result<(), AttachmentError>;
}

impl<H: AttachmentHost + ?Sized> AttachmentStore for H {
    fn holds(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
    ) -> Result<bool, AttachmentError> {
        Ok(Attachments::holds(
            self.attachments(holder)?,
            namespace,
            name,
        ))
    }
    fn decoded(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
    ) -> Result<Option<AttachmentValue>, AttachmentError> {
        Attachments::decoded(self.attachments(holder)?, namespace, name)
    }
    fn saved(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
    ) -> Result<Option<Compound>, AttachmentError> {
        Attachments::saved(self.attachments(holder)?, namespace, name)
    }
    fn store(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
        value: AttachmentValue,
        form: AttachmentForm,
    ) -> Result<(), AttachmentError> {
        let AttachmentForm {
            encoded,
            policy,
            write,
        } = form;
        Attachments::store(
            self.attachments(holder)?,
            namespace,
            name,
            Decoded {
                value,
                encoded,
                policy,
            },
            write,
        );
        Ok(())
    }
    fn discard(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
        sync: bool,
    ) -> Result<(), AttachmentError> {
        Attachments::discard(self.attachments(holder)?, namespace, name, sync);
        Ok(())
    }
    fn queue_sync(
        &mut self,
        holder: Holder,
        namespace: &str,
        name: &str,
    ) -> Result<(), AttachmentError> {
        Attachments::queue_sync(self.attachments(holder)?, namespace, name);
        Ok(())
    }
}

/// The optional on-disk representation of an attachment.
pub struct AttachmentCodec<T> {
    pub encode: fn(&T) -> Result<Compound, AttachmentError>,
    pub decode: fn(&Compound) -> Result<T, AttachmentError>,
}

/// A mod-defined attachment key and its value's behavior.
pub struct AttachmentType<T: 'static> {
    pub namespace: &'static str,
    pub name: &'static str,
    pub default: fn() -> T,
    pub codec: Option<AttachmentCodec<T>>,
    pub copy_on_death: bool,
    /// Send the value to the clients that see the holder; needs a codec.
    pub sync: bool,
}

impl<T> AttachmentType<T> {
    fn policy(&self) -> AttachmentPolicy {
        AttachmentPolicy {
            copy_on_death: self.copy_on_death,
            sync: self.sync,
        }
    }
    fn decode(&self, saved: &Compound) -> Result<T, AttachmentError> {
        (self.codec.as_ref().ok_or(AttachmentError::Decode)?.decode)(saved)
    }
    fn validate_policy(&self) -> Result<(), AttachmentError> {
        if self.copy_on_death && self.codec.is_none() {
            return Err(AttachmentError::InvalidCopyPolicy);
        }
        if self.sync && self.codec.is_none() {
            return Err(AttachmentError::InvalidSyncPolicy);
        }
        Ok(())
    }
}

/// One attachment's new state for a client; `None` means removed.
#[derive(Clone, Debug, PartialEq)]
pub struct AttachmentSync {
    pub namespace: JavaString,
    pub name: JavaString,
    pub value: Option<Compound>,
}

impl<T: Clone + Send + Sync + 'static> AttachmentType<T> {
    /// Read a value, inserting its default if absent; see `get_existing`.
    pub fn get<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<T, AttachmentError> {
        if let Some(value) = self.get_existing(store, holder)? {
            return Ok(value);
        }
        let value = (self.default)();
        self.keep(store, holder, value.clone(), AttachmentWrite::Default)?;
        Ok(value)
    }
    /// Whether a holder carries a value, like NeoForge's `hasData`; never inserts the default.
    pub fn has<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<bool, AttachmentError> {
        store.holds(holder, self.namespace, self.name)
    }
    /// Read a value only if present, like `getExistingData`; a saved value decodes once.
    pub fn get_existing<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<Option<T>, AttachmentError> {
        self.validate_policy()?;
        if let Some(value) = store.decoded(holder, self.namespace, self.name)? {
            return value
                .downcast_ref::<T>()
                .cloned()
                .map(Some)
                .ok_or(AttachmentError::TypeMismatch);
        }
        let Some(saved) = store.saved(holder, self.namespace, self.name)? else {
            return Ok(None);
        };
        let value = self.decode(&saved)?;
        self.keep(store, holder, value.clone(), AttachmentWrite::Decoded)?;
        Ok(Some(value))
    }
    /// Replace a value on an existing holder. A key already decoded as a different type is
    /// rejected.
    pub fn set<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
        value: T,
    ) -> Result<(), AttachmentError> {
        self.validate_policy()?;
        if let Some(old) = store.decoded(holder, self.namespace, self.name)?
            && !old.is::<T>()
        {
            return Err(AttachmentError::TypeMismatch);
        }
        self.keep(store, holder, value, AttachmentWrite::Set)
    }
    /// Remove a value from an existing holder. A saved value that does not decode is removed
    /// too, and its error returned.
    pub fn remove<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<Option<T>, AttachmentError> {
        if !self.has(store, holder)? {
            return Ok(None);
        }
        let value = self.get_existing(store, holder);
        store.discard(holder, self.namespace, self.name, self.sync)?;
        value
    }
    /// Send the current value to the clients that see the holder.
    pub fn sync_data<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
    ) -> Result<(), AttachmentError> {
        self.validate_policy()?;
        if self.sync {
            store.queue_sync(holder, self.namespace, self.name)?;
        }
        Ok(())
    }
    fn keep<S: AttachmentStore + ?Sized>(
        &self,
        store: &mut S,
        holder: Holder,
        value: T,
        write: AttachmentWrite,
    ) -> Result<(), AttachmentError> {
        let encoded = self
            .codec
            .as_ref()
            .map(|codec| (codec.encode)(&value))
            .transpose()?;
        let value: AttachmentValue = Arc::new(value);
        let form = AttachmentForm {
            encoded,
            policy: self.policy(),
            write,
        };
        store.store(holder, self.namespace, self.name, value, form)
    }
}

/// A decoded value with its saved form and its type's policy.
struct Decoded {
    value: AttachmentValue,
    encoded: Option<Compound>,
    policy: AttachmentPolicy,
}

/// The saved form of a decoded value: its encoding and the policies it was
/// declared with. `None` when the type has no codec.
fn envelope(value: &Decoded) -> Option<Compound> {
    value.encoded.as_ref().map(|encoded| {
        let mut envelope = Compound::new();
        envelope.insert("value".into(), Tag::Compound(encoded.clone()));
        if value.policy.copy_on_death {
            envelope.insert("copy_on_death".into(), Tag::Byte(1));
        }
        if value.policy.sync {
            envelope.insert("sync".into(), Tag::Byte(1));
        }
        envelope
    })
}
fn raw_value(raw: &Compound) -> Option<&Compound> {
    match raw.get(&"value".into()) {
        Some(Tag::Compound(value)) => Some(value),
        _ => None,
    }
}

enum Entry {
    Raw { saved: Compound, reads: ReadCache },
    Decoded(Decoded),
}

/// An immutable attachment binding shared by a holder and its snapshots.
/// Query reads may cache decoded values, but never change the canonical raw/decoded state.
#[doc(hidden)]
#[non_exhaustive]
pub struct AttachmentEntry {
    state: Entry,
}

// Separate Rust types can read the same raw binding without claiming its canonical type.
// Each successful type gets a stable borrowed value; failed decodes are not cached.
#[derive(Default)]
struct ReadCache {
    value: OnceLock<AttachmentValue>,
    next: OnceLock<Box<ReadCache>>,
}

impl AttachmentEntry {
    fn raw(saved: Compound) -> Self {
        Self {
            state: Entry::Raw {
                saved,
                reads: ReadCache::default(),
            },
        }
    }
    /// Borrow a value for a query without inserting a default, storing a decoded value,
    /// or changing saved policies or sync state. Decoded bindings never fall back to NBT.
    pub fn read<T: Send + Sync + 'static>(
        &self,
        key: &AttachmentType<T>,
    ) -> Result<&T, AttachmentError> {
        key.validate_policy()?;
        match &self.state {
            Entry::Decoded(value) => value
                .value
                .downcast_ref()
                .ok_or(AttachmentError::TypeMismatch),
            Entry::Raw { saved, reads } => {
                let mut cache = reads;
                loop {
                    let value = match cache.value.get() {
                        Some(value) => value,
                        None => {
                            let value = Arc::new(
                                key.decode(raw_value(saved).ok_or(AttachmentError::Decode)?)?,
                            );
                            // Another reader may have initialized this slot with its type.
                            let _ = cache.value.set(value);
                            cache.value.get().expect("cache initialized")
                        }
                    };
                    if let Some(value) = value.downcast_ref() {
                        return Ok(value);
                    }
                    cache = cache.next.get_or_init(Default::default);
                }
            }
        }
    }
    /// The encoded value, as clients receive it.
    fn synced_value(&self) -> Option<&Compound> {
        match &self.state {
            Entry::Raw { saved, .. } => raw_value(saved),
            Entry::Decoded(value) => value.encoded.as_ref(),
        }
    }
    fn is_synced(&self) -> bool {
        match &self.state {
            Entry::Raw { saved, .. } => matches!(saved.get(&"sync".into()), Some(Tag::Byte(1))),
            Entry::Decoded(value) => value.policy.sync,
        }
    }
}

/// The data held by one entity, block entity, chunk or level.
#[derive(Clone, Default)]
pub struct Attachments {
    entries: BTreeMap<(JavaString, JavaString), Arc<AttachmentEntry>>,
    // Keep malformed or otherwise unrecognized NBT exactly as it was read.
    // Empty holders need no shared allocation.
    raw: Option<Arc<Compound>>,
    // Keys to send to clients at the next flush; never saved.
    pending: BTreeSet<(JavaString, JavaString)>,
}
impl fmt::Debug for Attachments {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Attachments")
            .field("keys", &self.entries.keys())
            .finish()
    }
}

/// A lone map answers for any holder.
impl AttachmentHost for Attachments {
    fn attachments(&mut self, _: Holder) -> Result<&mut Attachments, AttachmentError> {
        Ok(self)
    }
}

impl Attachments {
    /// Read the namespaced attachment compound, leaving unknown data intact.
    pub fn from_nbt(raw: Compound) -> Self {
        let mut result = Self::default();
        for (namespace, tag) in &raw {
            if let Tag::Compound(names) = tag {
                for (name, value) in names {
                    if let Tag::Compound(value) = value {
                        result.entries.insert(
                            (namespace.clone(), name.clone()),
                            Arc::new(AttachmentEntry::raw(value.clone())),
                        );
                    }
                }
            }
        }
        result.raw = (!raw.is_empty()).then(|| Arc::new(raw));
        result
    }
    /// Encode changed values, preserving raw entries and unrecognized tags.
    pub fn to_nbt(&self) -> Compound {
        let mut raw = self.raw.as_deref().cloned().unwrap_or_default();
        for ((namespace, name), entry) in &self.entries {
            let value = match &entry.state {
                Entry::Raw { saved, .. } => Some(saved.clone()),
                Entry::Decoded(value) => envelope(value),
            };
            if let Some(value) = value {
                let tag = raw
                    .entry(namespace.clone())
                    .or_insert_with(|| Tag::Compound(Compound::new()));
                let Tag::Compound(names) = tag else {
                    unreachable!("attachment namespace checked before mutation");
                };
                names.insert(name.clone(), Tag::Compound(value));
            }
        }
        raw
    }
    /// Copy opted-in values through their disk encoding for a player respawn.
    pub fn copy_on_death(&self) -> Result<Self, AttachmentError> {
        let mut copy = Self::default();
        for (key, entry) in &self.entries {
            let encoded = match &entry.state {
                Entry::Raw { saved, .. }
                    if matches!(saved.get(&"copy_on_death".into()), Some(Tag::Byte(1))) =>
                {
                    Some(saved.clone())
                }
                Entry::Decoded(value) if value.policy.copy_on_death => {
                    Some(envelope(value).ok_or(AttachmentError::InvalidCopyPolicy)?)
                }
                _ => None,
            };
            if let Some(encoded) = encoded {
                copy.entries
                    .insert(key.clone(), Arc::new(AttachmentEntry::raw(encoded)));
            }
        }
        Ok(copy)
    }
    /// The shared immutable binding, without decoding or inserting a value.
    #[doc(hidden)]
    pub fn entry(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<&Arc<AttachmentEntry>>, AttachmentError> {
        self.check_namespace(namespace)?;
        Ok(self.entries.get(&(namespace.into(), name.into())))
    }
    /// Whether a value is stored, saved or set, without decoding or inserting it.
    fn holds(&self, namespace: &str, name: &str) -> bool {
        self.entries.contains_key(&(namespace.into(), name.into()))
    }
    /// Refuse a namespace whose saved form isn't a compound: nothing under it can be read or
    /// written without losing it.
    fn check_namespace(&self, namespace: &str) -> Result<(), AttachmentError> {
        match self.raw.as_ref().and_then(|raw| raw.get(&namespace.into())) {
            Some(tag) if !matches!(tag, Tag::Compound(_)) => Err(AttachmentError::InvalidNamespace),
            _ => Ok(()),
        }
    }
    fn decoded(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<AttachmentValue>, AttachmentError> {
        Ok(
            match self.entry(namespace, name)?.map(|entry| &entry.state) {
                Some(Entry::Decoded(value)) => Some(value.value.clone()),
                _ => None,
            },
        )
    }
    fn saved(&self, namespace: &str, name: &str) -> Result<Option<Compound>, AttachmentError> {
        match self.entry(namespace, name)?.map(|entry| &entry.state) {
            Some(Entry::Raw { saved, .. }) => raw_value(saved)
                .cloned()
                .map(Some)
                .ok_or(AttachmentError::Decode),
            _ => Ok(None),
        }
    }
    fn store(&mut self, namespace: &str, name: &str, value: Decoded, write: AttachmentWrite) {
        let sync = value.policy.sync;
        let key: (JavaString, JavaString) = (namespace.into(), name.into());
        self.entries.insert(
            key.clone(),
            Arc::new(AttachmentEntry {
                state: Entry::Decoded(value),
            }),
        );
        if sync && write != AttachmentWrite::Decoded {
            self.pending.insert(key);
        }
        if write == AttachmentWrite::Set {
            self.remove_saved(namespace, name);
        }
    }
    fn discard(&mut self, namespace: &str, name: &str, sync: bool) {
        let key: (JavaString, JavaString) = (namespace.into(), name.into());
        self.entries.remove(&key);
        if sync {
            self.pending.insert(key);
        }
        self.remove_saved(namespace, name);
    }
    fn remove_saved(&mut self, namespace: &str, name: &str) {
        if let Some(raw) = &mut self.raw
            && let Some(Tag::Compound(names)) = Arc::make_mut(raw).get_mut(&namespace.into())
        {
            names.remove(&name.into());
        }
    }
    fn queue_sync(&mut self, namespace: &str, name: &str) {
        self.pending.insert((namespace.into(), name.into()));
    }
    /// The changes to send to clients since the last call: the current value
    /// of each queued key, or `None` for one that is gone.
    pub fn take_sync_changes(&mut self) -> Vec<AttachmentSync> {
        std::mem::take(&mut self.pending)
            .into_iter()
            .map(|(namespace, name)| {
                let value = self
                    .entries
                    .get(&(namespace.clone(), name.clone()))
                    .and_then(|entry| entry.synced_value())
                    .cloned();
                AttachmentSync {
                    namespace,
                    name,
                    value,
                }
            })
            .collect()
    }
    /// Every synced value, for a client that starts seeing the holder.
    pub fn synced(&self) -> Vec<AttachmentSync> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.is_synced())
            .filter_map(|((namespace, name), entry)| {
                Some(AttachmentSync {
                    namespace: namespace.clone(),
                    name: name.clone(),
                    value: Some(entry.synced_value()?.clone()),
                })
            })
            .collect()
    }
    /// Client side: keep what the server sent, decoded by the next `get`.
    pub fn apply_sync(&mut self, change: AttachmentSync) {
        let key = (change.namespace, change.name);
        match change.value {
            Some(value) => {
                let raw = [("value".into(), Tag::Compound(value))].into();
                self.entries
                    .insert(key, Arc::new(AttachmentEntry::raw(raw)));
            }
            None => {
                self.entries.remove(&key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(n: &i32) -> Result<Compound, AttachmentError> {
        Ok([(JavaString::from("value"), Tag::Int(*n))].into())
    }
    fn decode(c: &Compound) -> Result<i32, AttachmentError> {
        match c.get(&"value".into()) {
            Some(Tag::Int(n)) => Ok(*n),
            _ => Err(AttachmentError::Decode),
        }
    }
    static COUNTER: AttachmentType<i32> = AttachmentType {
        namespace: "test",
        name: "counter",
        default: || 0,
        codec: Some(AttachmentCodec { encode, decode }),
        copy_on_death: false,
        sync: false,
    };
    static SYNCED: AttachmentType<i32> = AttachmentType {
        namespace: "test",
        name: "synced",
        default: || 7,
        codec: Some(AttachmentCodec { encode, decode }),
        copy_on_death: false,
        sync: true,
    };
    fn change(name: &str, value: Option<i32>) -> AttachmentSync {
        AttachmentSync {
            namespace: "test".into(),
            name: name.into(),
            value: value.map(|n| encode(&n).unwrap()),
        }
    }
    static MEMORY: AttachmentType<i32> = AttachmentType {
        namespace: "test",
        name: "memory",
        default: || 3,
        codec: None,
        copy_on_death: false,
        sync: false,
    };
    #[test]
    fn empty_and_new_maps_need_no_saved_backing_allocation() {
        let mut map = Attachments::default();
        assert!(map.raw.is_none());
        COUNTER.set(&mut map, Holder::Level, 1).unwrap();
        assert!(map.raw.is_none());
        COUNTER.remove(&mut map, Holder::Level).unwrap();
        assert!(map.raw.is_none());
        assert!(Attachments::from_nbt(Compound::new()).raw.is_none());
    }
    #[test]
    fn shared_bindings_and_map_snapshots_keep_replaced_and_removed_values() {
        let mut map = Attachments::default();
        SYNCED.set(&mut map, Holder::Level, 1).unwrap();
        map.take_sync_changes();
        let loaded = Attachments::from_nbt(map.to_nbt());
        for mut map in [map, loaded] {
            let mut snapshot = map.clone();
            let entry = map.entry("test", "synced").unwrap().unwrap().clone();
            assert!(Arc::ptr_eq(
                &entry,
                snapshot.entry("test", "synced").unwrap().unwrap()
            ));
            let saved = snapshot.to_nbt();

            SYNCED.set(&mut map, Holder::Level, 2).unwrap();
            assert_eq!(entry.read(&SYNCED), Ok(&1));
            assert_eq!(SYNCED.get_existing(&mut map, Holder::Level), Ok(Some(2)));
            assert_eq!(SYNCED.remove(&mut map, Holder::Level), Ok(Some(2)));
            assert!(map.entry("test", "synced").unwrap().is_none());
            assert_eq!(entry.read(&SYNCED), Ok(&1));
            assert_eq!(
                SYNCED.get_existing(&mut snapshot, Holder::Level),
                Ok(Some(1))
            );
            assert_eq!(snapshot.to_nbt(), saved);
            assert!(snapshot.take_sync_changes().is_empty());
            assert_eq!(map.take_sync_changes(), [change("synced", None)]);
        }
    }
    #[test]
    fn raw_read_cache_borrows_without_claiming_the_canonical_type_or_policy() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static DECODES: AtomicUsize = AtomicUsize::new(0);
        struct NonClone(i32);
        let key = AttachmentType {
            namespace: "test",
            name: "counter",
            default: || panic!("a query must not create a default"),
            codec: Some(AttachmentCodec {
                encode: |_: &NonClone| panic!("a query must not encode"),
                decode: |saved| {
                    DECODES.fetch_add(1, Ordering::Relaxed);
                    decode(saved).map(NonClone)
                },
            }),
            copy_on_death: true,
            sync: true,
        };
        let mut original = Attachments::default();
        COUNTER.set(&mut original, Holder::Level, 42).unwrap();
        let saved = original.to_nbt();
        let mut map = Attachments::from_nbt(saved.clone());
        let snapshot = map.clone();
        assert!(Arc::ptr_eq(
            map.raw.as_ref().unwrap(),
            snapshot.raw.as_ref().unwrap()
        ));
        let entry = map.entry("test", "counter").unwrap().unwrap().clone();
        let value = entry.read(&key).unwrap();
        assert_eq!(value.0, 42);
        assert!(std::ptr::eq(value, entry.read(&key).unwrap()));
        assert!(std::ptr::eq(
            value,
            snapshot
                .entry("test", "counter")
                .unwrap()
                .unwrap()
                .read(&key)
                .unwrap()
        ));
        assert_eq!(DECODES.load(Ordering::Relaxed), 1);
        assert!(map.decoded("test", "counter").unwrap().is_none());
        assert_eq!(map.to_nbt(), saved);
        assert!(map.synced().is_empty());
        assert!(map.take_sync_changes().is_empty());
        assert!(
            map.copy_on_death()
                .unwrap()
                .entry("test", "counter")
                .unwrap()
                .is_none()
        );

        // Another query type gets its own borrowed cache; ordinary reads still decode raw.
        assert_eq!(entry.read(&COUNTER), Ok(&42));
        assert!(std::ptr::eq(value, entry.read(&key).unwrap()));
        assert_eq!(COUNTER.get_existing(&mut map, Holder::Level), Ok(Some(42)));
        let decoded = map.entry("test", "counter").unwrap().unwrap();
        assert!(matches!(
            decoded.read(&key),
            Err(AttachmentError::TypeMismatch)
        ));
        assert_eq!(map.to_nbt(), saved);
        assert!(map.take_sync_changes().is_empty());
        assert_eq!(entry.read(&key).unwrap().0, 42);
    }
    #[test]
    fn query_reads_reject_invalid_policy_without_populating_the_cache() {
        let mut map = Attachments::default();
        map.apply_sync(change("synced", Some(9)));
        let invalid = AttachmentType {
            codec: None,
            ..SYNCED
        };
        let entry = map.entry("test", "synced").unwrap().unwrap();
        assert_eq!(
            entry.read(&invalid),
            Err(AttachmentError::InvalidSyncPolicy)
        );
        assert_eq!(entry.read(&SYNCED), Ok(&9));
        assert!(map.take_sync_changes().is_empty());
    }
    #[test]
    fn failed_query_decodes_are_retried_without_changing_the_saved_binding() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
        let key = AttachmentType {
            codec: Some(AttachmentCodec {
                encode,
                decode: |saved| {
                    if ATTEMPTS.fetch_add(1, Ordering::Relaxed) == 0 {
                        Err(AttachmentError::Decode)
                    } else {
                        decode(saved)
                    }
                },
            }),
            ..SYNCED
        };
        let mut map = Attachments::default();
        map.apply_sync(change("synced", Some(9)));
        let saved = map.to_nbt();
        let entry = map.entry("test", "synced").unwrap().unwrap();
        assert_eq!(entry.read(&key), Err(AttachmentError::Decode));
        assert_eq!(entry.read(&key), Ok(&9));
        assert_eq!(entry.read(&key), Ok(&9));
        assert_eq!(ATTEMPTS.load(Ordering::Relaxed), 2);
        assert!(map.decoded("test", "synced").unwrap().is_none());
        assert_eq!(map.to_nbt(), saved);
        assert!(map.take_sync_changes().is_empty());
    }
    #[test]
    fn sync_replacement_and_removal_leave_the_shared_old_entry_readable() {
        let mut map = Attachments::default();
        map.apply_sync(change("synced", Some(9)));
        let old = map.entry("test", "synced").unwrap().unwrap().clone();
        map.apply_sync(change("synced", Some(10)));
        assert_eq!(old.read(&SYNCED), Ok(&9));
        assert_eq!(SYNCED.get_existing(&mut map, Holder::Level), Ok(Some(10)));
        map.apply_sync(change("synced", None));
        assert_eq!(old.read(&SYNCED), Ok(&9));
        assert_eq!(SYNCED.get_existing(&mut map, Holder::Level), Ok(None));
    }
    #[test]
    fn saved_values_round_trip_and_memory_values_do_not() {
        let mut map = Attachments::default();
        COUNTER.set(&mut map, Holder::Level, 42).unwrap();
        MEMORY.set(&mut map, Holder::Level, 8).unwrap();
        let mut loaded = Attachments::from_nbt(map.to_nbt());
        assert_eq!(COUNTER.get(&mut loaded, Holder::Level), Ok(42));
        assert_eq!(MEMORY.get(&mut loaded, Holder::Level), Ok(3));
        assert_eq!(COUNTER.remove(&mut loaded, Holder::Level), Ok(Some(42)));
        let mut reopened = Attachments::from_nbt(loaded.to_nbt());
        assert_eq!(COUNTER.get(&mut reopened, Holder::Level), Ok(0));
    }
    #[test]
    fn untouched_saved_value_copies_on_respawn() {
        let keep = AttachmentType {
            namespace: "test",
            name: "keep",
            default: || 0,
            codec: Some(AttachmentCodec { encode, decode }),
            copy_on_death: true,
            sync: false,
        };
        let mut original = Attachments::default();
        keep.set(&mut original, Holder::Level, 42).unwrap();
        COUNTER.set(&mut original, Holder::Level, 5).unwrap();
        let loaded = Attachments::from_nbt(original.to_nbt());
        let mut respawned = loaded.copy_on_death().unwrap();
        assert_eq!(keep.get(&mut respawned, Holder::Level), Ok(42));
        assert_eq!(COUNTER.get(&mut respawned, Holder::Level), Ok(0));
    }
    #[test]
    fn existing_reads_leave_an_absent_value_absent() {
        let mut map = Attachments::default();
        assert!(!COUNTER.has(&mut map, Holder::Level).unwrap());
        assert_eq!(COUNTER.get_existing(&mut map, Holder::Level), Ok(None));
        assert!(!COUNTER.has(&mut map, Holder::Level).unwrap());
        assert_eq!(map.to_nbt(), Compound::new());
        assert_eq!(COUNTER.get(&mut map, Holder::Level), Ok(0));
        assert!(COUNTER.has(&mut map, Holder::Level).unwrap());
        assert_ne!(map.to_nbt(), Compound::new());
        COUNTER.set(&mut map, Holder::Level, 5).unwrap();
        assert_eq!(COUNTER.get_existing(&mut map, Holder::Level), Ok(Some(5)));
    }
    #[test]
    fn existing_read_decodes_a_saved_value_without_changing_it() {
        let mut map = Attachments::default();
        COUNTER.set(&mut map, Holder::Level, 42).unwrap();
        MEMORY.set(&mut map, Holder::Level, 8).unwrap();
        let saved = map.to_nbt();
        let mut loaded = Attachments::from_nbt(saved.clone());
        assert!(COUNTER.has(&mut loaded, Holder::Level).unwrap());
        assert_eq!(
            COUNTER.get_existing(&mut loaded, Holder::Level),
            Ok(Some(42))
        );
        assert_eq!(loaded.to_nbt(), saved);
        assert!(!MEMORY.has(&mut loaded, Holder::Level).unwrap());
        assert_eq!(MEMORY.get_existing(&mut loaded, Holder::Level), Ok(None));
    }
    #[test]
    fn unknown_and_invalid_values_survive_a_failed_decode() {
        let saved: Compound = [(
            JavaString::from("test"),
            Tag::Compound(
                [
                    (
                        JavaString::from("counter"),
                        Tag::Compound(
                            [(JavaString::from("value"), Tag::String("bad".into()))].into(),
                        ),
                    ),
                    (JavaString(vec![0xd800]), Tag::Compound(Compound::new())),
                ]
                .into(),
            ),
        )]
        .into();
        let mut map = Attachments::from_nbt(saved.clone());
        let old = map.entry("test", "counter").unwrap().unwrap().clone();
        assert_eq!(old.read(&COUNTER), Err(AttachmentError::Decode));
        assert!(COUNTER.has(&mut map, Holder::Level).unwrap());
        assert_eq!(
            COUNTER.get_existing(&mut map, Holder::Level),
            Err(AttachmentError::Decode)
        );
        assert_eq!(
            COUNTER.get(&mut map, Holder::Level),
            Err(AttachmentError::Decode)
        );
        assert_eq!(map.to_nbt(), saved);
        assert_eq!(
            COUNTER.remove(&mut map, Holder::Level),
            Err(AttachmentError::Decode)
        );
        assert_eq!(
            COUNTER.remove(&mut map, Holder::Level),
            Ok(None),
            "removed all the same"
        );
        assert_eq!(old.read(&COUNTER), Err(AttachmentError::Decode));
    }
    #[test]
    fn synced_set_and_remove_report_the_last_state_once() {
        let mut map = Attachments::default();
        SYNCED.set(&mut map, Holder::Level, 1).unwrap();
        SYNCED.set(&mut map, Holder::Level, 2).unwrap();
        SYNCED.remove(&mut map, Holder::Level).unwrap();
        assert_eq!(map.take_sync_changes(), [change("synced", None)]);
        assert!(map.take_sync_changes().is_empty());
    }
    #[test]
    fn default_creation_is_synced_and_a_later_read_is_not() {
        let mut map = Attachments::default();
        assert_eq!(SYNCED.get(&mut map, Holder::Level), Ok(7));
        assert_eq!(map.take_sync_changes(), [change("synced", Some(7))]);
        assert_eq!(SYNCED.get(&mut map, Holder::Level), Ok(7));
        assert!(map.take_sync_changes().is_empty());
    }
    #[test]
    fn unsynced_types_report_no_changes() {
        let mut map = Attachments::default();
        COUNTER.set(&mut map, Holder::Level, 1).unwrap();
        COUNTER.remove(&mut map, Holder::Level).unwrap();
        COUNTER.sync_data(&mut map, Holder::Level).unwrap();
        assert!(map.take_sync_changes().is_empty());
    }
    #[test]
    fn sync_without_a_codec_is_refused() {
        let broken = AttachmentType {
            namespace: "test",
            name: "broken",
            default: || 0,
            codec: None,
            copy_on_death: false,
            sync: true,
        };
        let mut map = Attachments::default();
        assert_eq!(
            broken.get(&mut map, Holder::Level),
            Err(AttachmentError::InvalidSyncPolicy)
        );
        assert_eq!(
            broken.set(&mut map, Holder::Level, 1),
            Err(AttachmentError::InvalidSyncPolicy)
        );
        assert_eq!(
            broken.sync_data(&mut map, Holder::Level),
            Err(AttachmentError::InvalidSyncPolicy)
        );
        assert!(map.take_sync_changes().is_empty());
    }
    #[test]
    fn a_saved_sync_flag_puts_undecoded_values_in_the_snapshot() {
        let mut map = Attachments::default();
        SYNCED.set(&mut map, Holder::Level, 5).unwrap();
        COUNTER.set(&mut map, Holder::Level, 6).unwrap();
        let saved = map.to_nbt();
        let Some(Tag::Compound(names)) = saved.get(&"test".into()) else {
            panic!("namespace saved");
        };
        let Some(Tag::Compound(envelope)) = names.get(&"synced".into()) else {
            panic!("entry saved");
        };
        assert_eq!(envelope.get(&"sync".into()), Some(&Tag::Byte(1)));
        let loaded = Attachments::from_nbt(saved);
        assert_eq!(loaded.synced(), [change("synced", Some(5))]);
    }
    #[test]
    fn applied_changes_decode_through_the_codec_and_removals_clear() {
        let mut map = Attachments::default();
        map.apply_sync(change("synced", Some(9)));
        assert_eq!(SYNCED.get(&mut map, Holder::Level), Ok(9));
        map.apply_sync(change("synced", None));
        assert_eq!(SYNCED.get(&mut map, Holder::Level), Ok(7));
    }
    #[test]
    fn sync_data_reports_the_current_value() {
        struct One(Attachments);
        impl AttachmentHost for One {
            fn attachments(&mut self, _: Holder) -> Result<&mut Attachments, AttachmentError> {
                Ok(&mut self.0)
            }
        }
        let mut host = One(Attachments::default());
        SYNCED.set(&mut host, Holder::Level, 4).unwrap();
        host.0.take_sync_changes();
        SYNCED.sync_data(&mut host, Holder::Level).unwrap();
        assert_eq!(host.0.take_sync_changes(), [change("synced", Some(4))]);
    }
}
