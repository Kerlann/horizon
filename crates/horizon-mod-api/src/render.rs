//! Render hook points for native mods (docs/modding/src/how-to/native-mods.md): empty functions the renderer calls at
//! fixed places of a frame, which a mod's mixins hook like those of
//! `horizon-client/src/game/native_hooks.rs`, and declared with them in `hooks/`.

use glam::{DVec3, Mat4};
use horizon_client_core::subscribers::Subscribers;
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};

mod generated {
    include!(concat!(env!("OUT_DIR"), "/native_render_generated.rs"));
}
pub use generated::{
    HandDrawnEvent, RenderHookId, RenderHooks, RenderNeedsEvent, WorldDrawnEvent, render_hook_ids,
};
#[doc(hidden)]
pub use horizon_mod_api_macros::__subscriptions;
pub use horizon_mod_api_macros::horizon_mod;
pub use wgpu;

static RENDERERS: OnceLock<RenderInstalled> = OnceLock::new();

/// A renderer and its owner's index in the installed mod dispatch order.
// exhaustive: private fields; construction records ownership explicitly, never by trait-object identity.
pub struct RenderEntry {
    mod_index: usize,
    hooks: &'static dyn RenderHooks,
}

impl RenderEntry {
    /// Bind a renderer to its mod's index once, when the game installs its mods.
    pub fn new(mod_index: usize, hooks: &'static dyn RenderHooks) -> Self {
        Self { mod_index, hooks }
    }
}

/// The renderers with their per-hook subscriber set, as [`install`] leaves them.
struct RenderInstalled {
    renderers: Box<[RenderEntry]>,
    subscribers: Subscribers,
}

#[cfg(any(test, feature = "testing"))]
thread_local! {
    static TEST_RENDERERS: std::cell::Cell<Option<&'static RenderInstalled>> =
        const { std::cell::Cell::new(None) };
}

fn render_installed() -> Option<&'static RenderInstalled> {
    #[cfg(any(test, feature = "testing"))]
    if let Some(installed) = TEST_RENDERERS.get() {
        return Some(installed);
    }
    RENDERERS.get()
}

fn new_render_installed(renderers: Box<[RenderEntry]>) -> RenderInstalled {
    let subscriptions = renderers.iter().enumerate().flat_map(|(index, renderer)| {
        renderer
            .hooks
            .subscribed()
            .iter()
            .flatten()
            .map(move |hook| (index, *hook as usize))
    });
    let subscribers =
        Subscribers::new(generated::RENDER_HOOK_COUNT, renderers.len(), subscriptions);
    RenderInstalled {
        renderers,
        subscribers,
    }
}

/// Install trait mod renderers in dispatch order, once per process.
pub fn install(renderers: Box<[RenderEntry]>) -> Result<(), Box<[RenderEntry]>> {
    match RENDERERS.set(new_render_installed(renderers)) {
        Ok(()) => Ok(()),
        Err(installed) => Err(installed.renderers),
    }
}

/// The renderers subscribed to `hook`, in installation order.
fn entered_render(
    hook_id: generated::RenderHookId,
) -> impl Iterator<Item = &'static dyn RenderHooks> {
    render_installed().into_iter().flat_map(move |installed| {
        installed
            .subscribers
            .indices(hook_id as usize)
            .filter(move |&index| crate::mod_index_on(installed.renderers[index].mod_index))
            .map(move |index| installed.renderers[index].hooks)
    })
}

/// Whether a render hook has a subscriber.
fn render_listened(hook: generated::RenderHookId) -> bool {
    render_installed().is_some_and(|installed| installed.subscribers.contains_any(hook as usize))
}

static MODS_BUILT_IN: AtomicBool = AtomicBool::new(false);

/// The game was built with native mods (the game sets it at start): one of them may draw in
/// [`native_world_drawn`], so the first-person hand keeps its own pass after it.
pub fn set_mods_built_in(on: bool) {
    MODS_BUILT_IN.store(on, Ordering::Relaxed);
}
/// Whether [`native_world_drawn`] may draw ([`set_mods_built_in`]).
pub fn world_drawn_hooked() -> bool {
    MODS_BUILT_IN.load(Ordering::Relaxed)
        || render_installed().is_some_and(|installed| !installed.renderers.is_empty())
}

/// What a mod can draw with once the world is drawn: after the sky, terrain, entities,
/// particles, weather and the world post pass, before the first-person hand ([`HandDrawn`]),
/// the HUD and the screens. `color` holds the world's picture (copy it out before reading it: it is the target
/// drawn into), `depth` the world's depth, reversed (0 is the sky).
#[non_exhaustive]
pub struct WorldDrawn<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// The frame's target and a view of it in `color_format`.
    pub color: &'a wgpu::Texture,
    pub color_view: &'a wgpu::TextureView,
    pub color_format: wgpu::TextureFormat,
    /// The size of `color`.
    pub size: [u32; 2],
    /// The world's depth (Depth32Float, sampleable), and its size: smaller than `size` when the
    /// world draws at a lower resolution. It holds the frame's depth only when a mod asked for
    /// it ([`RenderNeeds::world_depth`]) or something else kept it.
    pub depth: &'a wgpu::TextureView,
    pub depth_size: [u32; 2],
    /// Camera-relative world to clip space, without TAA's jitter, and with it (what the world
    /// was drawn with).
    pub view_proj: Mat4,
    pub jittered: Mat4,
    /// The camera's position in the world.
    pub camera: DVec3,
    /// The camera-relative entity geometry of the frame (`Frame::entities`), quads of 4
    /// vertices, and its runs by entity when a mod asked for them
    /// ([`RenderNeeds::entity_spans`]) or a shader pack draws them.
    pub entities: &'a [Vertex],
    pub spans: &'a [EntitySpan],
    /// Which entity each run of `entities` is, recorded every frame (`Frame::entity_motion`):
    /// the run's first vertex and the entity, `(1 << 32) | id` for a mob or a player and
    /// `(2 << 32) | id` for a dropped item (their entity ids), 0 for anything else. A run lasts
    /// until the next. Unlike `spans`, whose ids name a type, these follow one entity from
    /// frame to frame.
    pub motion: &'a [(u32, u64)],
    /// The shared atlas entity UVs point into.
    pub atlas: Option<&'a wgpu::Texture>,
    /// Quad indices (0 1 2 0 2 3 for each 4 vertices), for as many quads as `entities` holds.
    pub quad_indices: &'a wgpu::Buffer,
    /// The block textures the terrain samples, for drawing blocks with the player's own
    /// textures (`native_block_sprites` says where a block's are).
    pub block_sprites: BlockSpriteArrays<'a>,
}

/// The block sprite arrays ([`WorldDrawn::block_sprites`]): each texture a whole square layer,
/// its UVs 0 to 1 across it, as the world uses them (gamma-encoded, animated layers at the frame
/// shown).
#[non_exhaustive]
pub struct BlockSpriteArrays<'a> {
    /// The arrays as `D2Array` views, terrain.wgsl's eight (`sprites`, `sprites_1`...): a 1-layer
    /// stand-in past the ones in use, each with the mip levels the Mipmap Levels option leaves.
    pub views: &'a [wgpu::TextureView],
    /// Layers per array on this device (2048 on most; fewer where the GPU holds fewer, and
    /// OpenGL pads each array past it): a sprite's `array` and `layer` are cut by it.
    pub split: u32,
    /// A layer's width and height in texels.
    pub size: u32,
    /// Nearest, repeating, the nearest mip level: texels drawn sharp.
    pub nearest: &'a wgpu::Sampler,
    /// The terrain's own (linear, clamped, anisotropic under that Texture Filtering).
    pub filtered: &'a wgpu::Sampler,
}

/// What a mod can draw with once the first-person hand is drawn ([`native_hand_drawn`]): after
/// the world and the hand, before entity outlines, post effects, screen effects and the HUD.
/// `color` holds the frame's picture with the hand in it (copy it out before reading it).
#[non_exhaustive]
pub struct HandDrawn<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// The frame's target and a view of it in `color_format`.
    pub color: &'a wgpu::Texture,
    pub color_view: &'a wgpu::TextureView,
    pub color_format: wgpu::TextureFormat,
    /// The size of `color`.
    pub size: [u32; 2],
    /// The hand and held item just drawn, camera-relative quads of 4 vertices: empty when no
    /// hand drew (third person, F1, a sleeping or spectating player) or a shader pack drew it.
    pub hand: &'a [Vertex],
    /// Camera-relative world to clip space for the hand: its own field of view
    /// (`hand_fov_degrees`) and the view bob, without TAA's jitter (the hand has none). Its
    /// depth is the hand's own: the hand's pass clears depth and keeps none.
    pub view_proj: Mat4,
    /// The camera's position in the world.
    pub camera: DVec3,
    /// The shared atlas the hand's UVs point into.
    pub atlas: Option<&'a wgpu::Texture>,
    /// Quad indices (0 1 2 0 2 3 for each 4 vertices).
    pub quad_indices: &'a wgpu::Buffer,
}

/// What the mods need from a frame that the game doesn't make for itself. Each mod sets what it
/// needs and leaves the rest, so several can ask.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RenderNeeds {
    /// Record `Frame::entity_spans` without a shader pack: entity runs get a stable id from
    /// their type's name, block entity runs their block state.
    pub entity_spans: bool,
    /// Keep the world pass's depth for [`native_world_drawn`] when nothing else does.
    pub world_depth: bool,
}

/// The world is drawn (see [`WorldDrawn`]). Not called on the title screen's panorama.
#[inline(never)]
pub fn native_world_drawn(world: &mut WorldDrawn<'_>) {
    generated::dispatch_world_drawn(world);
}

/// The first-person hand is drawn (see [`HandDrawn`]), on every frame [`native_world_drawn`]
/// runs, with no hand too.
#[inline(never)]
pub fn native_hand_drawn(hand: &mut HandDrawn<'_>) {
    generated::dispatch_hand_drawn(hand);
}

/// Asked as a frame is built and drawn: set what your mod needs.
#[inline(never)]
pub fn native_render_needs(needs: &mut RenderNeeds) {
    generated::dispatch_render_needs(needs);
}

/// What the mods need this frame ([`native_render_needs`]).
pub fn render_needs() -> RenderNeeds {
    let mut needs = RenderNeeds::default();
    native_render_needs(&mut needs);
    needs
}

#[cfg(test)]
mod tests {
    use super as mod_api;
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Install renderers for this test thread, as [`install`] does once per process.
    fn install_for_test(renderers: Box<[&'static dyn RenderHooks]>) -> &'static RenderInstalled {
        let renderers = renderers
            .iter()
            .enumerate()
            .map(|(index, &hooks)| RenderEntry::new(index, hooks))
            .collect();
        let installed = Box::leak(Box::new(new_render_installed(renderers)));
        TEST_RENDERERS.set(Some(installed));
        installed
    }

    struct Depth;
    #[mod_api::__subscriptions]
    impl RenderHooks for Depth {
        fn render_needs(&self, event: &mut RenderNeedsEvent<'_>) {
            event.needs.world_depth = true;
        }
    }
    struct Spans;
    #[mod_api::__subscriptions]
    impl RenderHooks for Spans {
        fn render_needs(&self, event: &mut RenderNeedsEvent<'_>) {
            event.needs.entity_spans = event.needs.world_depth;
        }
    }
    struct Quiet;
    #[mod_api::__subscriptions]
    impl RenderHooks for Quiet {}
    struct CountingNeeds(Arc<AtomicUsize>);
    #[mod_api::__subscriptions]
    impl RenderHooks for CountingNeeds {
        fn render_needs(&self, _event: &mut RenderNeedsEvent<'_>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn shared_renderer_identity_does_not_merge_mod_owners() {
        use crate as mod_api;

        struct Owner;
        #[crate::__subscriptions]
        impl crate::Hooks for Owner {}
        let mods = vec![
            crate::ModEntry::new("first", Box::new(Owner)),
            crate::ModEntry::new("second", Box::new(Owner)),
        ]
        .into();
        crate::testing::with_mods(mods, || {
            let count = Arc::new(AtomicUsize::new(0));
            let hooks = Box::leak(Box::new(CountingNeeds(count.clone())));
            // Both owners intentionally return the very same trait object.
            let renderers = vec![RenderEntry::new(0, hooks), RenderEntry::new(1, hooks)].into();
            let installed = Box::leak(Box::new(new_render_installed(renderers)));
            TEST_RENDERERS.set(Some(installed));
            assert!(crate::__private::set_mod_enabled("first", false));
            native_render_needs(&mut RenderNeeds::default());
            assert_eq!(count.load(Ordering::Relaxed), 1);
            assert!(crate::__private::set_mod_enabled("second", false));
            native_render_needs(&mut RenderNeeds::default());
            assert_eq!(count.load(Ordering::Relaxed), 1);
            assert!(crate::__private::set_mod_enabled("first", true));
            native_render_needs(&mut RenderNeeds::default());
            assert_eq!(count.load(Ordering::Relaxed), 2);
        });
        TEST_RENDERERS.set(None);
    }

    #[test]
    fn needs_accumulate_in_mod_order() {
        static DEPTH: Depth = Depth;
        static SPANS: Spans = Spans;
        install_for_test(Box::new([&DEPTH, &SPANS] as [&dyn RenderHooks; 2]));
        let mut needs = RenderNeeds::default();
        native_render_needs(&mut needs);
        assert_eq!(
            needs,
            RenderNeeds {
                world_depth: true,
                entity_spans: true
            }
        );
    }

    /// Eight defaults and one override resolve to one subscriber at installation.
    #[test]
    fn render_subscribers_are_known_at_install() {
        let renderers: Vec<&'static dyn RenderHooks> = (0..8)
            .map(|_| Box::leak(Box::new(Quiet)) as &'static dyn RenderHooks)
            .collect();
        let calls = Arc::new(AtomicUsize::new(0));
        let listener: &'static CountingNeeds = Box::leak(Box::new(CountingNeeds(calls.clone())));
        let renderers = {
            let mut renderers = renderers;
            renderers.push(listener);
            renderers.into_boxed_slice()
        };
        let installed = install_for_test(renderers);
        assert_eq!(
            installed
                .subscribers
                .indices(generated::RenderHookId::RenderNeeds as usize)
                .collect::<Vec<_>>(),
            [8]
        );
        let mut needs = RenderNeeds::default();
        native_render_needs(&mut needs);
        native_render_needs(&mut needs);
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        assert_eq!(
            installed
                .subscribers
                .indices(generated::RenderHookId::RenderNeeds as usize)
                .collect::<Vec<_>>(),
            [8]
        );
    }

    /// Render hooks use the same installed set from another thread.
    #[test]
    fn render_dispatch_uses_the_installed_set_on_another_thread() {
        let renderers: Vec<&'static dyn RenderHooks> = (0..4)
            .map(|_| Box::leak(Box::new(Quiet)) as &'static dyn RenderHooks)
            .collect();
        let calls = Arc::new(AtomicUsize::new(0));
        let listener: &'static CountingNeeds = Box::leak(Box::new(CountingNeeds(calls.clone())));
        let renderers = {
            let mut renderers = renderers;
            renderers.push(listener);
            renderers.into_boxed_slice()
        };
        let installed = install_for_test(renderers);
        let needs = RenderNeeds::default();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                // The render thread installs nothing itself: it dispatches the process-wide table.
                TEST_RENDERERS.set(Some(installed));
                let mut needs = needs;
                native_render_needs(&mut needs);
            });
        });
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            installed
                .subscribers
                .indices(generated::RenderHookId::RenderNeeds as usize)
                .collect::<Vec<_>>(),
            [4]
        );
    }

    thread_local! {
        static NEEDS_EVENTS_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    impl Drop for RenderNeedsEvent<'_> {
        fn drop(&mut self) {
            NEEDS_EVENTS_BUILT.set(NEEDS_EVENTS_BUILT.get() + 1);
        }
    }

    /// With no subscriber, dispatch returns before building its event.
    #[test]
    fn empty_render_dispatch_builds_no_event() {
        let mut needs = RenderNeeds::default();
        install_for_test(Box::new([]));
        NEEDS_EVENTS_BUILT.set(0);
        native_render_needs(&mut needs);
        assert_eq!(NEEDS_EVENTS_BUILT.get(), 0);
        static QUIET: Quiet = Quiet;
        install_for_test(Box::new([&QUIET as &dyn RenderHooks]));
        native_render_needs(&mut needs);
        assert_eq!(NEEDS_EVENTS_BUILT.get(), 0);
        native_render_needs(&mut needs);
        native_render_needs(&mut needs);
        assert_eq!(NEEDS_EVENTS_BUILT.get(), 0);
    }

    #[test]
    #[ignore = "optimized hot-hook microbenchmark"]
    fn render_needs_benchmark() {
        use std::hint::black_box;
        use std::time::Instant;

        let renderers: Vec<&'static dyn RenderHooks> = (0..8)
            .map(|_| Box::leak(Box::new(Quiet)) as &'static dyn RenderHooks)
            .collect();
        let calls = Arc::new(AtomicUsize::new(0));
        let listener: &'static CountingNeeds = Box::leak(Box::new(CountingNeeds(calls.clone())));
        let renderers = {
            let mut renderers = renderers;
            renderers.push(listener);
            renderers.into_boxed_slice()
        };
        let before: Box<[&'static dyn RenderHooks]> = renderers.clone();
        install_for_test(renderers);
        native_render_needs(&mut RenderNeeds::default()); // Warm dispatch outside the timed loop.
        let iterations = 1_000_000;
        let start = Instant::now();
        for _ in 0..iterations {
            let mut needs = RenderNeeds::default();
            let mut event = RenderNeedsEvent { needs: &mut needs };
            for renderer in black_box(&before) {
                renderer.render_needs(&mut event);
            }
        }
        let baseline = start.elapsed().as_nanos() as f64 / f64::from(iterations);
        let start = Instant::now();
        for _ in 0..iterations {
            let mut needs = RenderNeeds::default();
            native_render_needs(&mut needs);
        }
        let subscribed = start.elapsed().as_nanos() as f64 / f64::from(iterations);
        // With no subscriber left, dispatch returns after its count check.
        let renderers: Box<[&'static dyn RenderHooks]> = (0..9)
            .map(|_| Box::leak(Box::new(Quiet)) as &'static dyn RenderHooks)
            .collect();
        install_for_test(renderers);
        native_render_needs(&mut RenderNeeds::default()); // Warm the empty dispatch.
        let start = Instant::now();
        for _ in 0..iterations {
            let mut needs = RenderNeeds::default();
            native_render_needs(&mut needs);
        }
        let empty = start.elapsed().as_nanos() as f64 / f64::from(iterations);
        println!(
            "render_needs dispatch: before {baseline:.2} ns, subscribed {subscribed:.2} ns, empty {empty:.2} ns per call ({iterations} iterations)"
        );
    }
}
/// An entity, item and effect vertex (the entity pipelines), with a face
/// normal for `entity.wgsl`'s directional light.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
// exhaustive: the game builds it with a literal; mods only read it
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub layer: u32,
    /// RGBA8: tint multiplied by directional shade and ambient occlusion.
    pub color: u32,
    /// Smooth light, 16x fixed point: sky in bits 0..8, block in bits 8..16.
    pub light: u32,
    /// RGBA8_SNORM face normal; w = -127 for two-sided lighting, 0 to keep baked/unlit colour,
    /// `CULL_BACK` to keep it and also drop back-facing pixels.
    pub normal: [i8; 4],
    /// The `GLINT_TEXTURING` atlas UV for a foil item quad; zero elsewhere.
    pub glint_uv: [f32; 2],
}
/// Where a run of `Frame::entities` begins, and what it is (slice 7): the run lasts until the
/// next span. Recorded only while a shader pack draws entities, so Horizon's own frames carry
/// none; Horizon itself still draws the whole list at once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
// exhaustive: the game builds it with a literal; mods only read it
pub struct EntitySpan {
    /// The first vertex.
    pub start: u32,
    /// The render stage (5.4: `ENTITIES`, `BLOCK_ENTITIES`, `PARTICLES`, `DESTROY`), which picks
    /// the program.
    pub stage: u32,
    /// `entityId` for entities, `blockEntityId` for block entities (5.3), else 0.
    pub id: i32,
    /// An entity part that blends (a name tag, a shadow): `gbuffers_entities_translucent`.
    pub translucent: bool,
    /// Hidden from the camera (Horizon's entity culling, the player in first person): only the
    /// shadow pass draws it.
    pub hidden: bool,
    /// The local player (`shadowPlayer`).
    pub player: bool,
    /// A block entity whose block emits light (`shadowLightBlockEntities`).
    pub emissive: bool,
}
impl EntitySpan {
    /// Marks vertices `start..end` of `spans`' list (one entity) as hidden: the span covering
    /// `start` is split there, the spans inside are hidden, and the geometry after `end` goes
    /// on in the last span's kind. False when no span covers `start`.
    pub fn hide(spans: &mut Vec<EntitySpan>, start: usize, end: usize) -> bool {
        let Some(i) = spans.iter().rposition(|s| s.start as usize <= start) else {
            return false;
        };
        let after = spans[spans.len() - 1];
        if spans[i].start as usize != start {
            let split = EntitySpan {
                start: start as u32,
                ..spans[i]
            };
            spans.insert(i + 1, split);
        }
        for s in spans.iter_mut().skip(i) {
            if s.start as usize >= start {
                s.hidden = true;
            }
        }
        if end > start {
            spans.push(EntitySpan {
                start: end as u32,
                ..after
            });
        }
        true
    }
}

#[doc(hidden)]
pub(crate) mod built {
    #[allow(unused_imports)]
    use super::*;
    // exhaustive: the game builds it with a literal; mods only read it
    pub struct WorldDrawn<'a> {
        pub device: &'a wgpu::Device,
        pub queue: &'a wgpu::Queue,
        pub encoder: &'a mut wgpu::CommandEncoder,
        pub color: &'a wgpu::Texture,
        pub color_view: &'a wgpu::TextureView,
        pub color_format: wgpu::TextureFormat,
        pub size: [u32; 2],
        pub depth: &'a wgpu::TextureView,
        pub depth_size: [u32; 2],
        pub view_proj: Mat4,
        pub jittered: Mat4,
        pub camera: DVec3,
        pub entities: &'a [super::Vertex],
        pub spans: &'a [super::EntitySpan],
        pub motion: &'a [(u32, u64)],
        pub atlas: Option<&'a wgpu::Texture>,
        pub quad_indices: &'a wgpu::Buffer,
        pub block_sprites: super::BlockSpriteArrays<'a>,
    }
    impl<'a> From<WorldDrawn<'a>> for super::WorldDrawn<'a> {
        fn from(v: WorldDrawn<'a>) -> Self {
            let WorldDrawn {
                device,
                queue,
                encoder,
                color,
                color_view,
                color_format,
                size,
                depth,
                depth_size,
                view_proj,
                jittered,
                camera,
                entities,
                spans,
                motion,
                atlas,
                quad_indices,
                block_sprites,
            } = v;
            Self {
                device,
                queue,
                encoder,
                color,
                color_view,
                color_format,
                size,
                depth,
                depth_size,
                view_proj,
                jittered,
                camera,
                entities,
                spans,
                motion,
                atlas,
                quad_indices,
                block_sprites,
            }
        }
    }
    // exhaustive: the game builds it with a literal; mods only read it
    pub struct BlockSpriteArrays<'a> {
        pub views: &'a [wgpu::TextureView],
        pub split: u32,
        pub size: u32,
        pub nearest: &'a wgpu::Sampler,
        pub filtered: &'a wgpu::Sampler,
    }
    impl<'a> From<BlockSpriteArrays<'a>> for super::BlockSpriteArrays<'a> {
        fn from(v: BlockSpriteArrays<'a>) -> Self {
            let BlockSpriteArrays {
                views,
                split,
                size,
                nearest,
                filtered,
            } = v;
            Self {
                views,
                split,
                size,
                nearest,
                filtered,
            }
        }
    }
    // exhaustive: the game builds it with a literal; mods only read it
    pub struct HandDrawn<'a> {
        pub device: &'a wgpu::Device,
        pub queue: &'a wgpu::Queue,
        pub encoder: &'a mut wgpu::CommandEncoder,
        pub color: &'a wgpu::Texture,
        pub color_view: &'a wgpu::TextureView,
        pub color_format: wgpu::TextureFormat,
        pub size: [u32; 2],
        pub hand: &'a [super::Vertex],
        pub view_proj: Mat4,
        pub camera: DVec3,
        pub atlas: Option<&'a wgpu::Texture>,
        pub quad_indices: &'a wgpu::Buffer,
    }
    impl<'a> From<HandDrawn<'a>> for super::HandDrawn<'a> {
        fn from(v: HandDrawn<'a>) -> Self {
            let HandDrawn {
                device,
                queue,
                encoder,
                color,
                color_view,
                color_format,
                size,
                hand,
                view_proj,
                camera,
                atlas,
                quad_indices,
            } = v;
            Self {
                device,
                queue,
                encoder,
                color,
                color_view,
                color_format,
                size,
                hand,
                view_proj,
                camera,
                atlas,
                quad_indices,
            }
        }
    }
}
