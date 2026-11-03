//! Render hook points for native mods (docs/native-mods.md): empty functions the renderer calls at
//! fixed places of a frame, which a mod's mixins hook like those of
//! `horizon-client/src/game/native_hooks.rs`, and listed with them in `native-hooks.json`.

use super::Vertex;
use super::pack_draw::EntitySpan;
use glam::{DVec3, Mat4};
use horizon_client_core::subscribers::{Domain, Subscribers, enter};
use std::any::TypeId;
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};

mod generated {
    include!(concat!(env!("OUT_DIR"), "/native_render_generated.rs"));
}
pub use generated::{HandDrawnEvent, RenderHooks, RenderNeedsEvent, WorldDrawnEvent};
pub use wgpu;

static RENDERERS: OnceLock<RenderInstalled> = OnceLock::new();

/// The renderers with their per-hook subscriber set, as [`install`] leaves them.
struct RenderInstalled {
    renderers: Box<[&'static dyn RenderHooks]>,
    subscribers: Subscribers,
    types: Box<[TypeId]>,
}

#[cfg(test)]
thread_local! {
    static TEST_RENDERERS: std::cell::Cell<Option<&'static RenderInstalled>> =
        const { std::cell::Cell::new(None) };
}

fn render_installed() -> Option<&'static RenderInstalled> {
    #[cfg(test)]
    if let Some(installed) = TEST_RENDERERS.get() {
        return Some(installed);
    }
    RENDERERS.get()
}

fn new_render_installed(renderers: Box<[&'static dyn RenderHooks]>) -> RenderInstalled {
    let types = renderers
        .iter()
        .map(|renderer| (&**renderer as &dyn std::any::Any).type_id())
        .collect();
    let subscribers = Subscribers::new(
        Domain::Render,
        generated::RENDER_HOOK_COUNT,
        renderers.len(),
    );
    RenderInstalled {
        renderers,
        subscribers,
        types,
    }
}

/// Install trait mod renderers in dispatch order, once per process.
pub fn install(
    renderers: Box<[&'static dyn RenderHooks]>,
) -> Result<(), Box<[&'static dyn RenderHooks]>> {
    match RENDERERS.set(new_render_installed(renderers)) {
        Ok(()) => Ok(()),
        Err(installed) => Err(installed.renderers),
    }
}

/// The renderers still subscribed to `hook`, each marked as the receiver of the current
/// dispatch call, so its generated default can prune itself. The generated dispatch loops
/// over this; render hooks are called bare, without a crash guard.
fn entered_render(
    hook_id: generated::RenderHookId,
) -> impl Iterator<Item = &'static dyn RenderHooks> {
    render_installed().into_iter().flat_map(move |installed| {
        let mut guard = None;
        installed
            .subscribers
            .indices(hook_id as usize)
            .map(move |index| {
                drop(guard.take());
                let renderer = installed.renderers[index];
                guard = Some(enter(
                    &installed.subscribers,
                    hook_id as usize,
                    index,
                    std::ptr::from_ref::<dyn RenderHooks>(renderer).cast(),
                    installed.types[index],
                ));
                renderer
            })
    })
}

/// Whether a render hook still has a subscriber. Once false, this stays false.
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
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Install renderers for this test thread, as [`install`] does once per process.
    fn install_for_test(renderers: Box<[&'static dyn RenderHooks]>) -> &'static RenderInstalled {
        let installed = Box::leak(Box::new(new_render_installed(renderers)));
        TEST_RENDERERS.set(Some(installed));
        installed
    }

    struct Depth;
    impl RenderHooks for Depth {
        fn render_needs(&self, event: &mut RenderNeedsEvent<'_>) {
            event.needs.world_depth = true;
        }
    }
    struct Spans;
    impl RenderHooks for Spans {
        fn render_needs(&self, event: &mut RenderNeedsEvent<'_>) {
            event.needs.entity_spans = event.needs.world_depth;
        }
    }
    struct Quiet;
    impl RenderHooks for Quiet {}
    struct CountingNeeds(Arc<AtomicUsize>);
    impl RenderHooks for CountingNeeds {
        fn render_needs(&self, _event: &mut RenderNeedsEvent<'_>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
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

    /// Eight renderers at their defaults and one override: the defaults prune themselves on
    /// the first dispatch, and only the override is called after that.
    #[test]
    fn render_defaults_prune_to_the_override() {
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

    /// Render hooks run on the render thread: dispatch from another thread prunes there too.
    #[test]
    fn render_dispatch_prunes_from_another_thread() {
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

    /// With no renderer, or none left after pruning, dispatch returns before building its event.
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
        assert_eq!(
            NEEDS_EVENTS_BUILT.get(),
            1,
            "the first call prunes the default"
        );
        native_render_needs(&mut needs);
        native_render_needs(&mut needs);
        assert_eq!(NEEDS_EVENTS_BUILT.get(), 1);
    }

    /// A renderer whose override reaches a gameplay default with the same numeric id, as a mod
    /// implementing both `Hooks` and `RenderHooks` does through an inherited `Hooks` method.
    struct BothTables(Arc<AtomicUsize>);
    impl RenderHooks for BothTables {
        fn render_needs(&self, _event: &mut RenderNeedsEvent<'_>) {
            self.0.fetch_add(1, Ordering::Relaxed);
            horizon_client_core::subscribers::default_called(
                Domain::Gameplay,
                generated::RenderHookId::RenderNeeds as usize,
                self,
            );
        }
    }

    #[test]
    fn a_gameplay_default_does_not_prune_a_render_override() {
        let calls = Arc::new(AtomicUsize::new(0));
        let both: &'static BothTables = Box::leak(Box::new(BothTables(calls.clone())));
        install_for_test(Box::new([both as &dyn RenderHooks]));
        let mut needs = RenderNeeds::default();
        for _ in 0..3 {
            native_render_needs(&mut needs);
        }
        assert_eq!(calls.load(Ordering::Relaxed), 3);
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
        native_render_needs(&mut RenderNeeds::default()); // Prune defaults once, outside the timed loop.
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
        native_render_needs(&mut RenderNeeds::default()); // Prune all nine once.
        let start = Instant::now();
        for _ in 0..iterations {
            let mut needs = RenderNeeds::default();
            native_render_needs(&mut needs);
        }
        let empty = start.elapsed().as_nanos() as f64 / f64::from(iterations);
        println!(
            "render_needs dispatch: before {baseline:.2} ns, subscribed {subscribed:.2} ns, pruned-empty {empty:.2} ns per call ({iterations} iterations)"
        );
    }

    /// A mod samples `BlockSpriteArrays` as the renderer hands them over: a layer of the second
    /// array through its view, texels sharp and repeating with the nearest sampler.
    #[test]
    fn a_pass_samples_a_sprite_layer_sharp() {
        let instance = crate::gfx::test_gpu::instance();
        let Some((device, queue)) =
            pollster::block_on(instance.request_adapter(&Default::default()))
                .ok()
                .and_then(|adapter| {
                    pollster::block_on(adapter.request_device(&Default::default())).ok()
                })
        else {
            return;
        };
        let gl = device.adapter_info().backend == wgpu::Backend::Gl;
        // two arrays of two 2 by 2 layers; array 1's layer 1 red, green / blue, white
        let layers = super::super::array_layers(2, gl);
        let quad = [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255; 4],
        ];
        let textures: Vec<wgpu::Texture> = (0..2)
            .map(|array| {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d {
                        width: 2,
                        height: 2,
                        depth_or_array_layers: layers,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let mut texels = vec![0u8; 16 * layers as usize];
                if array == 1 {
                    texels[16..32].copy_from_slice(quad.as_flattened());
                }
                queue.write_texture(
                    texture.as_image_copy(),
                    &texels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(8),
                        rows_per_image: Some(2),
                    },
                    texture.size(),
                );
                texture
            })
            .collect();
        let unused = textures[0].create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let views = super::super::sprite_views(&textures, &unused, Some(1));
        let nearest = device.create_sampler(&super::super::SPRITE_NEAREST);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                "@group(0) @binding(0) var sprites: texture_2d_array<f32>;
                 @group(0) @binding(1) var sprites_1: texture_2d_array<f32>;
                 @group(0) @binding(2) var nearest: sampler;
                 @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
                     return vec4(f32(i & 1u) * 4.0 - 1.0, f32(i >> 1u) * 4.0 - 1.0, 0.0, 1.0);
                 }
                 @fragment fn fs(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
                     // inside each texel, and past the edge (repeating); the last reads array 0
                     let uv = array(vec2(1.3, 0.2), vec2(0.7, 0.45), vec2(-0.8, 0.9), vec2(0.6, 0.6));
                     let x = u32(at.x);
                     if x == 4u {
                         return textureSampleLevel(sprites, nearest, vec2(0.3), 1, 0.0);
                     }
                     return textureSampleLevel(sprites_1, nearest, uv[x], 1, 0.0);
                 }"
                .into(),
            ),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&nearest),
                },
            ],
        });
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 5,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let out = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let view = target.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &out,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
                    rows_per_image: None,
                },
            },
            target.size(),
        );
        queue.submit([encoder.finish()]);
        out.slice(..).map_async(wgpu::MapMode::Read, |r| r.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let texels = out.slice(..).get_mapped_range().unwrap()[..20].to_vec();
        let mut expected = quad.as_flattened().to_vec();
        expected.extend([0, 0, 0, 0]);
        assert_eq!(texels, expected);
    }
}
