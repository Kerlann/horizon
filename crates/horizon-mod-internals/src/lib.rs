//! A wasm mod's declared game internals, generated at build time from its manifest and the
//! target game's catalog. Add this crate only when using `[native] internals` in a wasm mod.
//! `cargo horizon-mod build --wasm` supplies the manifest and catalog to the generator.
include!(concat!(env!("OUT_DIR"), "/view.rs"));

/// Generic call-frame machinery. API-specific wrappers and codecs are all generated.
#[doc(hidden)]
#[cfg(target_arch = "wasm32")]
pub mod runtime {
    pub use bindings::exports::horizon::mod_internals::raw_listeners::{Guest, HostKind};
    pub use bindings::horizon::mod_internals::view::{Arg, Node, Reached, Refusal, call, root};
    pub use mod_points::internals::bindings;
    pub use mod_points::internals::{Value, flatten, unflatten};
    use std::marker::PhantomData;

    pub fn changed(message: &str) -> Refusal {
        Refusal::Changed(message.into())
    }
    pub fn wire_refusal(why: mod_points::internals::Refusal) -> Refusal {
        match why {
            mod_points::internals::Refusal::Gone(s) => Refusal::Gone(s),
            mod_points::internals::Refusal::Changed(s) => Refusal::Changed(s),
            mod_points::internals::Refusal::NotHere(s) => Refusal::NotHere(s),
        }
    }
    pub trait CopyValue: Sized {
        fn to_value(&self) -> Value;
        fn from_value(value: Value) -> Result<Self, Refusal>;
    }
    pub trait ViewType: Sized {
        fn from_reached(reached: Reached) -> Result<Self, Refusal>;
    }
    pub fn root_as<T: ViewType>(index: u32) -> T {
        T::from_reached(Reached::Node(root(index).expect("raw call root"))).expect("raw root shape")
    }
    pub trait AsNode {
        fn node(&self) -> &Node;
    }

    /// A generated copy-field descriptor for a batched read or write.
    pub struct Field<'a, T> {
        node: &'a Node,
        index: u32,
        ty: PhantomData<T>,
    }
    impl<'a, T: CopyValue> Field<'a, T> {
        #[doc(hidden)]
        pub fn new(node: &'a Node, index: u32) -> Self {
            Self {
                node,
                index,
                ty: PhantomData,
            }
        }
        /// Pair this field with a value for `set_many`.
        pub fn write(self, value: T) -> Write<'a> {
            Write {
                node: self.node,
                index: self.index,
                cells: flatten(value.to_value()),
            }
        }
    }
    /// A typed value paired with its destination field.
    pub struct Write<'a> {
        node: &'a Node,
        index: u32,
        cells: Vec<bindings::horizon::mod_internals::view::Cell>,
    }
    /// A tuple of generated field descriptors (up to twelve fields).
    pub trait ReadFields {
        type Results;
        fn get(self) -> Self::Results;
    }
    /// A tuple of writes made by `Field::write` (up to twelve fields).
    pub trait WriteFields {
        type Results;
        fn set(self) -> Self::Results;
    }
    /// Read up to twelve declared fields in one crossing, with one result per entry.
    pub fn get_many<R: ReadFields>(fields: R) -> R::Results {
        fields.get()
    }
    /// Journal up to twelve writes in one crossing, with one result per entry.
    /// Writes join the listener's journal like single writes and are applied on listener return
    /// or before a declared thunk call, not immediately at this call.
    pub fn set_many<W: WriteFields>(writes: W) -> W::Results {
        writes.set()
    }
    macro_rules! batches {
        ($($t:ident:$i:tt),+) => {
            impl<$($t: CopyValue),+> ReadFields for ($(Field<'_, $t>,)+) {
                type Results = ($(Result<$t, Refusal>,)+);
                fn get(self) -> Self::Results {
                    let results = bindings::horizon::mod_internals::view::get_many(&[$((self.$i.node, self.$i.index)),+]);
                    let mut results = results.into_iter();
                    ($(results.next().expect("one result per field").and_then(|cells| {
                        $t::from_value(unflatten(cells).map_err(wire_refusal)?)
                    }),)+)
                }
            }
            impl WriteFields for ($(batches!(@write $t),)+) {
                type Results = ($(batches!(@result $t),)+);
                fn set(self) -> Self::Results {
                    let results = bindings::horizon::mod_internals::view::set_many(&[$((self.$i.node, self.$i.index, self.$i.cells)),+]);
                    let mut results = results.into_iter();
                    ($(batches!(@next results $t),)+)
                }
            }
        };
        (@write $t:ident) => { Write<'_> };
        (@result $t:ident) => { Result<(), Refusal> };
        (@next $r:ident $t:ident) => { $r.next().expect("one result per field") };
    }
    batches!(A:0);
    batches!(A:0, B:1);
    batches!(A:0, B:1, C:2);
    batches!(A:0, B:1, C:2, D:3);
    batches!(A:0, B:1, C:2, D:3, E:4);
    batches!(A:0, B:1, C:2, D:3, E:4, F:5);
    batches!(A:0, B:1, C:2, D:3, E:4, F:5, G:6);
    batches!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7);
    batches!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, J:8);
    batches!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, J:8, K:9);
    batches!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, J:8, K:9, L:10);
    batches!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, J:8, K:9, L:10, M:11);

    /// A sequence node; elements are copied or reached as call-scoped nodes by their generated type.
    pub struct Seq<T>(Node, PhantomData<T>);
    impl<T: ViewType> Seq<T> {
        pub fn len(&self) -> Result<u64, Refusal> {
            self.0.len()
        }
        pub fn is_empty(&self) -> Result<bool, Refusal> {
            Ok(self.len()? == 0)
        }
        pub fn get(&self, index: u64) -> Result<T, Refusal> {
            T::from_reached(self.0.index(index)?)
        }
    }
    impl<T> AsNode for Seq<T> {
        fn node(&self) -> &Node {
            &self.0
        }
    }
    impl<T> ViewType for Seq<T> {
        fn from_reached(reached: Reached) -> Result<Self, Refusal> {
            match reached {
                Reached::Node(n) => Ok(Self(n, PhantomData)),
                _ => Err(changed("a sequence node")),
            }
        }
    }
    /// A map node. Keys use the same generated copy codec as arguments and fields.
    pub struct Map<K, V>(Node, PhantomData<(K, V)>);
    impl<K: CopyValue, V: ViewType> Map<K, V> {
        pub fn len(&self) -> Result<u64, Refusal> {
            self.0.len()
        }
        pub fn is_empty(&self) -> Result<bool, Refusal> {
            Ok(self.len()? == 0)
        }
        pub fn get(&self, key: &K) -> Result<V, Refusal> {
            V::from_reached(self.0.key(&flatten(key.to_value()))?)
        }
    }
    impl<K, V> AsNode for Map<K, V> {
        fn node(&self) -> &Node {
            &self.0
        }
    }
    impl<K, V> ViewType for Map<K, V> {
        fn from_reached(reached: Reached) -> Result<Self, Refusal> {
            match reached {
                Reached::Node(n) => Ok(Self(n, PhantomData)),
                _ => Err(changed("a map node")),
            }
        }
    }
    /// A handle-typed option. A missing value is not a stale node.
    pub struct Opt<T>(Node, PhantomData<T>);
    impl<T: ViewType> Opt<T> {
        pub fn case(&self) -> Result<u32, Refusal> {
            self.0.case()
        }
        pub fn some(&self) -> Result<Option<T>, Refusal> {
            match self.case()? {
                0 => Ok(None),
                1 => Ok(Some(T::from_reached(Reached::Node(self.0.field(0)?))?)),
                _ => Err(changed("an option case")),
            }
        }
    }
    impl<T> AsNode for Opt<T> {
        fn node(&self) -> &Node {
            &self.0
        }
    }
    impl<T> ViewType for Opt<T> {
        fn from_reached(reached: Reached) -> Result<Self, Refusal> {
            match reached {
                Reached::Node(n) => Ok(Self(n, PhantomData)),
                _ => Err(changed("an option node")),
            }
        }
    }
}
