//! Exercise the generated proxies and service against the real guest dispatcher, without wasm.
#![allow(dead_code, unused_variables, unused_mut, unsafe_op_in_unsafe_fn)]
use mod_api::wire::{Id, Reader, WireError, Writer, guest};
use mod_api::{Hooks, LoopContext, StructurePiece};
use std::{rc::Rc, sync::Arc};
mod boundary {
    include!(concat!(env!("OUT_DIR"), "/wire_host_generated.rs"));
}
#[derive(Clone)]
struct WasmMod {
    subscribed: &'static [Option<mod_api::HookId>],
    corrupt: bool,
}
impl std::fmt::Debug for WasmMod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Loopback")
    }
}
#[derive(Debug)]
struct WasmHandler {
    index: Id,
    module: WasmMod,
}
#[derive(Clone, Copy)]
enum CallClass {
    Load,
    Hook,
    Worker,
    Callback,
}
trait NoAnswer {
    fn no_answer() -> Self;
}
impl<T: Default> NoAnswer for T {
    fn no_answer() -> Self {
        Self::default()
    }
}
fn call_refused<E: From<(mod_api::Identifier, String)>>() -> E {
    E::from((
        mod_api::Identifier::parse("test:refused").unwrap(),
        "refused".into(),
    ))
}
impl WasmHandler {
    fn subscriptions(&self, owner: &'static str, id: u32) -> &'static [Option<mod_api::HookId>] {
        let names = self
            .module
            .call(
                CallClass::Load,
                boundary::Frame::default(),
                id,
                Some(self.index),
                |_| {},
                |r| r.take(),
            )
            .unwrap();
        boundary::subscriptions_of(owner, names)
    }
}
impl WasmMod {
    fn call<R>(
        &self,
        _: CallClass,
        frame: boundary::Frame,
        id: u32,
        handler: Option<Id>,
        encode: impl FnOnce(&mut Writer),
        decode: impl FnOnce(&mut Reader<'_>) -> Result<R, WireError>,
    ) -> Option<R> {
        let mut cx = Box::new(boundary::Cx::default());
        cx.frames.push(frame);
        cx.module = Some(self.clone());
        let pointer = &mut *cx as *mut boundary::Cx;
        let _resources = unsafe { boundary::resource_scope(pointer) };
        // The pointer remains live until this synchronous call finishes; guest callbacks never retain it.
        guest::set_transport(Some(Rc::new(move |id, bytes| {
            let mut out = Writer::new();
            let _resources = unsafe { boundary::resource_scope(pointer) };
            unsafe { boundary::serve(id, pointer, bytes, &mut out) }.unwrap();
            out.into_bytes()
        })));
        let mut input = Writer::new();
        if let Some(handler) = handler {
            input.put(&handler);
        }
        encode(&mut input);
        if input.error().is_some() {
            return None;
        }
        let at = guest::input(input.bytes().len());
        unsafe {
            std::ptr::copy_nonoverlapping(input.bytes().as_ptr(), at, input.bytes().len());
        }
        let (at, len) = guest::run(id);
        guest::set_transport(None);
        if len == u32::MAX {
            return None;
        }
        let bytes = unsafe { std::slice::from_raw_parts(at, len as usize) };
        let bytes = if self.corrupt {
            &bytes[..bytes.len().saturating_sub(1)]
        } else {
            bytes
        };
        decode(&mut Reader::new(bytes)).ok()
    }
}
struct Context;
impl LoopContext for Context {
    fn side(&self) -> mod_api::LoopSide {
        mod_api::LoopSide::Server
    }
    fn registry(&self, _: &str) -> Option<Arc<[String]>> {
        None
    }
    fn setting(&self, _: &str, _: &str) -> Option<String> {
        None
    }
    fn mods(&self) -> mod_api::LoadedMods {
        unreachable!("not used by this fixture")
    }
}
#[unsafe(no_mangle)]
fn __horizon_mod_hooks() -> Box<dyn Hooks> {
    struct Mod;
    impl Hooks for Mod {
        fn register(
            &self,
            registrar: &mut dyn mod_api::Registrar,
            _: &mod_api::RegistrationContext,
        ) {
            REGISTERED_ITEM.set(Some(registrar.item_key("minecraft:stone")));
            let kind = registrar.block_entity_type("counter").blocks(&[]).build();
            registrar.block_entity_part(&kind, MachineBlock).unwrap();
        }
        fn subscribed(&self) -> &'static [Option<mod_api::HookId>] {
            &[]
        }
        fn item_stacked_on_other(
            &self,
            _: &dyn LoopContext,
            event: &mut mod_api::ItemStackedOnOtherEvent<'_, '_>,
        ) -> mod_api::Flow {
            *event.event.carried.0 = None;
            event.event.cancellation_result = true;
            mod_api::Flow::Continue
        }
        fn living_jump_power(
            &self,
            context: &dyn LoopContext,
            event: &mut mod_api::LivingJumpPowerEvent<'_>,
        ) {
            // This is a plain import made through the generated Guest, not a stubbed reply.
            assert!(matches!(context.side(), mod_api::LoopSide::Server));
            event.entity = 99;
            event.power = 7.25;
            event.kind = "must not write back";
        }
    }
    Box::new(Mod)
}
#[derive(Debug)]
struct Piece;
impl StructurePiece for Piece {
    fn bounding_box(&self) -> mod_api::BoundingBox {
        mod_api::BoundingBox::new([1, 2, 3], [4, 5, 6])
    }
    fn post_process(
        &self,
        _: &mut dyn mod_api::PieceContext,
    ) -> Result<(), mod_api::StructureError> {
        Ok(())
    }
    fn add_additional_save_data(&self, _: &mut mod_api::Compound) {}
}
struct MachineBlock;
impl mod_api::BlockPart for MachineBlock {
    fn block_subscribed(&self) -> &'static [Option<mod_api::HookId>] {
        &[]
    }
}
impl mod_api::BlockEntityPart for MachineBlock {
    fn block_entity_subscribed(&self) -> &'static [Option<mod_api::HookId>] {
        &[Some(mod_api::HookId::BlockEntityTick)]
    }
    fn block_entity(
        &self,
        level: mod_api::LevelSide<'_>,
        event: &mut mod_api::BlockEntityEvent<'_>,
    ) -> Option<mod_api::Compound> {
        assert_eq!(event.block, "test:machine");
        matches!(level, mod_api::LevelSide::Server(_)).then(mod_api::Compound::new)
    }
    fn block_entity_tick(
        &self,
        _: &mut dyn mod_api::ServerHost,
        event: &mut mod_api::BlockEntityTickEvent<'_>,
    ) {
        event.entity.data[0] += 3;
    }
}
#[test]
fn wire_loopback_machine_tick_uses_checked_snapshot_writeback() {
    use mod_api::BlockEntityPart;
    let id = guest::add_handler(vec![guest::view::<dyn mod_api::BlockEntityPart>(Arc::new(
        MachineBlock,
    ))]);
    let mut entity = mod_api::ModBlockEntity::default();
    entity.data = vec![5];
    let identity = entity.id().to_string();
    let born = entity.revision().identity();
    let proxy = WasmHandler {
        index: id,
        module: WasmMod {
            subscribed: &[],
            corrupt: false,
        },
    };
    proxy.block_entity_tick(
        &mut guest::Guest,
        &mut mod_api::BlockEntityTickEvent {
            pos: [1, 2, 3],
            entity: mod_api::ModBlockEntityView::new(&mut entity),
        },
    );
    assert_eq!(entity.data, vec![8]);
    assert_eq!(entity.id(), identity);
    assert_eq!(entity.revision().identity(), born);
    let before = entity.revision().changes();
    let bad = WasmHandler {
        index: id,
        module: WasmMod {
            subscribed: &[],
            corrupt: true,
        },
    };
    bad.block_entity_tick(
        &mut guest::Guest,
        &mut mod_api::BlockEntityTickEvent {
            pos: [1, 2, 3],
            entity: mod_api::ModBlockEntityView::new(&mut entity),
        },
    );
    assert_eq!(entity.data, vec![8]);
    assert_eq!(entity.revision().changes(), before);
}
#[test]
fn wire_loopback_nested_tuple_reference_writes_back() {
    let module = WasmMod {
        subscribed: &[],
        corrupt: false,
    };
    install_counter();
    let mut carried = Some(mod_api::ItemStack::new(REGISTERED_ITEM.get().unwrap(), 1).unwrap());
    let mut clicked = guest::Guest;
    let mut stack = mod_api::StackedOnOther {
        player: None,
        player_entity_id: None,
        slot: 0,
        container: 0,
        container_slot: 0,
        primary: true,
        carried: mod_api::menu::CarriedAccess(&mut carried),
        clicked: &mut clicked,
        cancellation_result: false,
    };
    module.item_stacked_on_other(
        &Context,
        &mut mod_api::ItemStackedOnOtherEvent { event: &mut stack },
    );
    assert!(stack.carried.0.is_none());
    assert!(stack.cancellation_result);
}
thread_local! { static REGISTERED_ITEM:std::cell::Cell<Option<mod_api::ItemKey>> = const {std::cell::Cell::new(None)}; }
fn install_counter() {
    let mods = [mod_api::ModEntry::new(
        "wire_test",
        Box::new(WasmMod {
            subscribed: &[],
            corrupt: false,
        }),
    )];
    mod_api::__private::install_parts(&mods, &mod_api::__private::part_tests::rule_schema())
        .unwrap();
}
#[test]
fn wire_loopback_registration_attaches_subscribed_part() {
    install_counter();
    assert!(
        mod_api::__private::block_entity_types()
            .iter()
            .any(|decl| decl.id == "wire_test:counter")
    );
    assert!(mod_api::__private::has_part_hook(
        mod_api::HookId::BlockEntityTick
    ));
}
#[test]
fn wire_loopback_tagged_frame_enum_preserves_side() {
    use mod_api::BlockEntityPart;
    let index = guest::add_handler(vec![guest::view::<dyn mod_api::BlockEntityPart>(Arc::new(
        MachineBlock,
    ))]);
    let proxy = WasmHandler {
        index,
        module: WasmMod {
            subscribed: &[],
            corrupt: false,
        },
    };
    let mut event = mod_api::BlockEntityEvent {
        block: "test:machine",
    };
    assert!(
        proxy
            .block_entity(mod_api::LevelSide::Server(&guest::Guest), &mut event)
            .is_some()
    );
    assert!(
        proxy
            .block_entity(mod_api::LevelSide::Client(&guest::Guest), &mut event)
            .is_none()
    );
}
#[test]
fn wire_loopback_owned_box_handlers_inside_result_and_vec() {
    use mod_api::ReloadApply;
    let trace = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut out = Writer::new();
    {
        let _resources = guest::resource_scope();
        let trace = trace.clone();
        let value: Result<Box<dyn ReloadApply>, u32> = Ok(<dyn ReloadApply>::new(move || {
            trace.lock().unwrap().push(7)
        }));
        out.put_owned(value);
    }
    assert_eq!(out.error(), None);
    let mut cx = boundary::Cx::default();
    cx.module = Some(WasmMod {
        subscribed: &[],
        corrupt: false,
    });
    {
        let _resources = unsafe { boundary::resource_scope(&mut cx) };
        let value = Reader::decode::<Result<Box<dyn ReloadApply>, u32>>(out.bytes())
            .unwrap()
            .unwrap();
        value.apply();
    }
    assert_eq!(*trace.lock().unwrap(), vec![7]);
    out.clear();
    {
        let _resources = guest::resource_scope();
        let values = (0..3)
            .map(|n| {
                let trace = trace.clone();
                <dyn ReloadApply>::new(move || trace.lock().unwrap().push(n))
            })
            .collect::<Vec<_>>();
        out.put_owned(values);
    }
    assert_eq!(out.error(), None);
    {
        let _resources = unsafe { boundary::resource_scope(&mut cx) };
        for value in Reader::decode::<Vec<Box<dyn ReloadApply>>>(out.bytes()).unwrap() {
            value.apply();
        }
    }
    assert_eq!(*trace.lock().unwrap(), vec![7, 0, 1, 2]);
}
#[test]
fn wire_loopback_bad_import_bytes_are_typed_errors() {
    let mut cx = boundary::Cx::default();
    let mut out = Writer::new();
    let id = boundary::import_id("StructureRandom", "next_int_bound").unwrap();
    assert!(matches!(
        unsafe { boundary::serve(id, &mut cx, &[0xff], &mut out) },
        Err(boundary::ServeError::Decode(_))
    ));
    assert!(out.bytes().is_empty());
}
#[test]
fn wire_loopback_owner_thread_guards_access_and_drop() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Local {
        _local: Rc<()>,
        drops: Arc<AtomicUsize>,
    }
    impl Drop for Local {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    let drops = Arc::new(AtomicUsize::new(0));
    let local = boundary::OwnerThread::new(Local {
        _local: Rc::new(()),
        drops: drops.clone(),
    });
    assert!(local.get().is_ok());
    std::thread::spawn(move || {
        assert!(matches!(
            local.get(),
            Err(boundary::ServeError::WrongThread)
        ));
        drop(local);
    })
    .join()
    .unwrap();
    assert_eq!(
        drops.load(Ordering::SeqCst),
        0,
        "foreign-thread destruction must leak the thread-affine value"
    );
    drop(boundary::OwnerThread::new(Local {
        _local: Rc::new(()),
        drops: drops.clone(),
    }));
    assert_eq!(
        drops.load(Ordering::SeqCst),
        1,
        "creator-thread destruction must release the value"
    );
}
#[test]
fn wire_loopback_roundtrip_and_atomic_writeback() {
    let module = WasmMod {
        subscribed: &[],
        corrupt: false,
    };
    let mut event = mod_api::LivingJumpPowerEvent {
        entity: 42,
        kind: "original",
        power: 1.5,
    };
    module.living_jump_power(&Context, &mut event);
    assert_eq!(event.power, 7.25);
    assert_eq!(event.kind, "original");
    let bad = WasmMod {
        subscribed: &[],
        corrupt: true,
    };
    event.entity = 42;
    event.power = 2.0;
    bad.living_jump_power(&Context, &mut event);
    assert_eq!(
        event.power, 2.0,
        "a truncated last field must leave every host field untouched"
    );
    assert_eq!(event.entity, 42);
    let id = guest::add_handler(vec![guest::view::<dyn StructurePiece>(Arc::new(Piece))]);
    let proxy = WasmHandler { index: id, module };
    assert_eq!(
        proxy.bounding_box(),
        mod_api::BoundingBox::new([1, 2, 3], [4, 5, 6])
    );
    let mut release = Writer::new();
    release.put(&vec![id]);
    assert_eq!(
        proxy.module.call(
            CallClass::Load,
            boundary::Frame::default(),
            mod_api::wire::RELEASE,
            None,
            |w| w.put_bytes(release.bytes()),
            |r| r.take::<()>()
        ),
        Some(())
    );
    assert!(
        proxy
            .module
            .call::<mod_api::BoundingBox>(
                CallClass::Worker,
                boundary::Frame::default(),
                boundary::export_id("StructurePiece", "bounding_box").unwrap(),
                Some(id),
                |_| {},
                |r| r.take()
            )
            .is_none(),
        "released handler must be refused"
    );
}
