//! A screen's built state (`Screen.init`): made when the screen opens, remade
//! when the window or the GUI scale resizes it, dropped when it closes.
//!
//! Step 1 of #903 puts the seam in: every screen builds `BuildLayout::Empty`,
//! with the window's rectangle and nothing a screen draws or hit-tests yet.
//! The screens move onto it one at a time, and each one fills in its layout;
//! until it does, nothing reads the build, so no pixel moves.

use horizon_client::window::GuiSize;
use mod_api::ScreenView;

use super::{Game, Screen};

/// What the native hooks see of the screen showing (`observe_screen`), kept
/// beside its build so one `ScreenState` holds both.
pub(super) struct ObservedScreen {
    pub(super) id: &'static str,
    /// Not a `Screen` value: the title's own screen or an overlay drawn over
    /// gameplay (sign editor, book, dialog, prompt). `set_screen` never sees
    /// it open or close.
    pub(super) overlay: bool,
    pub(super) menu: Option<String>,
    pub(super) title: Option<std::sync::Arc<mod_api::Text>>,
    pub(super) rect: [f32; 4],
    pub(super) slots: usize,
    pub(super) mouse: [f32; 2],
    pub(super) partial_tick: f32,
}

impl ObservedScreen {
    pub(super) fn view(&self) -> ScreenView<'_> {
        ScreenView::from(mod_api::__private::built::ScreenView {
            id: self.id,
            menu: self.menu.as_deref(),
            title: self.title.as_deref(),
            rect: self.rect,
            slots: self.slots,
            mouse: self.mouse,
            partial_tick: self.partial_tick,
        })
    }
}

/// What a screen's drawing and its input read: its GUI-scaled size, the panel
/// it draws in, and its own widgets as rectangles.
///
/// Built once per open and per resize, never per frame. A screen bumps its own
/// revision when one of its layout inputs moves (Options' page or scroll, a
/// book opening), and the build is remade then.
#[allow(
    dead_code,
    reason = "step 1 of #903: screens read it as they move onto the build"
)]
pub(super) struct ScreenBuild {
    /// Which screen this was built for.
    pub(super) screen: Screen,
    identity: Option<&'static str>,
    /// The GUI-scaled size (`getGuiScaledWidth`/`getGuiScaledHeight`), in GUI pixels.
    pub(super) size: (f32, f32),
    /// The GUI scale (`narration`'s framebuffer space).
    pub(super) scale: f32,
    /// The panel the screen draws (`NativeScreenView.rect`), else the window.
    pub(super) rect: [f32; 4],
    /// The panel's `leftPos`/`topPos`.
    pub(super) origin: (f32, f32),
    /// The screen's own widgets, as rectangles.
    pub(super) layout: BuildLayout,
    /// Panels drawn outside `rect`.
    pub(super) exclusion: Vec<[f32; 4]>,
    /// `screen_init_pre` cancelled the screen's own widgets. Always false
    /// until that hook lands (step 7).
    pub(super) cancelled: bool,
}

/// A screen's own widgets, as rectangles.
///
/// Every screen is `Empty` in step 1; `Container`, `Options`, `Pause` and
/// `Chat` arrive with the screens that fill them.
#[derive(Clone, Debug, Default)]
pub(super) enum BuildLayout {
    /// Not migrated yet: the build has the window's rectangle only.
    #[default]
    Empty,
}

impl ScreenBuild {
    /// The build for `screen` at `gui`. Step 1's `Empty` screens know no panel
    /// and no widgets, so the rectangle is the window and the origin is zero.
    fn new(screen: Screen, identity: Option<&'static str>, gui: GuiSize) -> Self {
        let size = (gui.size[0] as f32, gui.size[1] as f32);
        Self {
            screen,
            identity,
            size,
            scale: gui.scale as f32,
            rect: [0.0, 0.0, size.0, size.1],
            origin: (0.0, 0.0),
            layout: BuildLayout::Empty,
            exclusion: Vec::new(),
            cancelled: false,
        }
    }
}

/// What the game knows about the screen showing: the snapshot the native hooks
/// see (`ObservedScreen`) and the build the screen's drawing will read.
#[derive(Default)]
pub(super) struct ScreenState {
    pub(super) observed: Option<ObservedScreen>,
    pub(super) build: ScreenBuildState,
}

/// The open screen's build and what it was made from.
#[derive(Default)]
pub(super) struct ScreenBuildState {
    current: Option<ScreenBuild>,
    #[cfg(test)]
    builds: u32,
}

impl ScreenBuildState {
    /// A fresh overlay instance must build even when its kind is unchanged.
    pub(super) fn reopened(&mut self) {
        self.current = None;
    }

    /// Reconciles the build with what shows now: a different screen or a
    /// different GUI size makes it, the same one keeps it, and nothing shown
    /// drops it. A call that changes nothing allocates nothing, so the frame
    /// path checks it every frame, and a menu swap on the same screen keeps it.
    pub(super) fn sync(
        &mut self,
        screen: Screen,
        shown: bool,
        identity: Option<&'static str>,
        gui: GuiSize,
    ) {
        if !shown {
            self.current = None;
            return;
        }
        let stale = self.current.as_ref().is_none_or(|build| {
            build.screen != screen
                || build.identity != identity
                || build.size != (gui.size[0] as f32, gui.size[1] as f32)
                || build.scale != gui.scale as f32
        });
        if stale {
            self.current = Some(ScreenBuild::new(screen, identity, gui));
            #[cfg(test)]
            {
                self.builds += 1;
            }
        }
    }
}

impl Game<'_> {
    /// The GUI-scaled size of the window and the scale it is drawn at, as the
    /// HUD computes it (`HudBatch::new`). Never asks for the renderer: before
    /// one exists, the window's own size serves.
    fn gui_size_now(&self) -> GuiSize {
        let framebuffer = self.renderer.as_ref().map_or_else(
            || {
                self.window.as_ref().map_or([1280, 720], |window| {
                    let size = window.inner_size();
                    [size.width, size.height]
                })
            },
            |renderer| [renderer.config.width, renderer.config.height],
        );
        let inputs = self.settings.gui_scale_inputs();
        GuiSize::new(framebuffer, inputs.requested, inputs.unicode)
    }

    /// `Screen.init`/`Screen.resize` for the game's open screen: builds it
    /// when it opens, remakes it when the window or the GUI scale changed it,
    /// and drops it when it closes. A frame that changes neither builds
    /// nothing, so `observe_screen` calls it every frame.
    pub(super) fn build_screen(&mut self) {
        let shown =
            self.screen != Screen::Playing || self.title.is_some() || self.screen_over_playing();
        let gui = self.gui_size_now();
        let identity = self
            .title
            .as_ref()
            .map(|title| title.mode.id())
            .or_else(|| {
                self.screen_over_playing()
                    .then(|| self.observed_screen_view().map(|view| view.id))
                    .flatten()
            });
        self.screen_state
            .build
            .sync(self.screen, shown, identity, gui);
    }
}

#[cfg(test)]
mod tests {
    use super::{BuildLayout, ScreenBuildState};
    use crate::game::Screen;
    use horizon_client::hud::{GuiScale, HudBatch};
    use horizon_client::window::GuiSize;

    fn gui(framebuffer: [u32; 2], scale: u32) -> GuiSize {
        GuiSize::new(framebuffer, scale, false)
    }

    /// One build per open and per resize; none for a frame that changed
    /// nothing, and none for a menu swapped on the same screen.
    #[test]
    fn a_build_per_open_and_resize_only() {
        let mut state = ScreenBuildState::default();
        let small = gui([854, 480], 0);
        state.sync(Screen::Inventory, true, None, small);
        assert_eq!(state.builds, 1);
        // Frames pass with the same screen and size.
        for _ in 0..3 {
            state.sync(Screen::Inventory, true, None, small);
        }
        assert_eq!(state.builds, 1);
        // A menu swapped on the same screen keeps the build.
        state.sync(Screen::Inventory, true, None, small);
        assert_eq!(state.builds, 1);
        // A window resize rebuilds once.
        let large = gui([1920, 1080], 0);
        state.sync(Screen::Inventory, true, None, large);
        assert_eq!(state.builds, 2);
        state.sync(Screen::Inventory, true, None, large);
        assert_eq!(state.builds, 2);
        // A GUI scale change rebuilds once.
        let scaled = GuiSize::new([1920, 1080], 3, false);
        state.sync(Screen::Inventory, true, None, scaled);
        assert_eq!(state.builds, 3);
        // A different screen opens.
        state.sync(Screen::Paused, true, None, scaled);
        assert_eq!(state.builds, 4);
    }

    #[test]
    fn distinct_title_and_overlay_identities_rebuild_at_the_same_size() {
        let mut state = ScreenBuildState::default();
        let size = gui([854, 480], 0);
        state.sync(Screen::Playing, true, Some("title"), size);
        assert_eq!(state.builds, 1);
        state.sync(Screen::Playing, true, Some("multiplayer"), size);
        assert_eq!(state.builds, 2);
        state.sync(Screen::Playing, true, Some("sign_editor"), size);
        assert_eq!(state.builds, 3);
        state.sync(Screen::Playing, true, Some("book_view"), size);
        assert_eq!(state.builds, 4);
        state.sync(Screen::Playing, true, Some("book_view"), size);
        assert_eq!(state.builds, 4);
        state.reopened();
        state.sync(Screen::Playing, true, Some("book_view"), size);
        assert_eq!(state.builds, 5);
    }

    #[test]
    fn closing_drops_the_build() {
        let mut state = ScreenBuildState::default();
        let size = gui([1280, 720], 0);
        state.sync(Screen::Paused, true, None, size);
        assert!(state.current.is_some());
        state.sync(Screen::Playing, false, None, size);
        assert!(state.current.is_none());
        assert_eq!(state.builds, 1);
        // Nothing open builds nothing.
        state.sync(Screen::Playing, false, None, size);
        assert_eq!(state.builds, 1);
    }

    /// A title or an overlay shows with `Screen::Playing` under it: it is still
    /// something to build for.
    #[test]
    fn a_title_or_overlay_over_playing_builds() {
        let mut state = ScreenBuildState::default();
        state.sync(Screen::Playing, true, None, gui([854, 480], 0));
        assert_eq!(state.builds, 1);
        assert_eq!(state.current.as_ref().unwrap().screen, Screen::Playing);
    }

    /// The build's size and scale are the ones the HUD draws at
    /// (`HudBatch::new`), at every window size and GUI scale.
    #[test]
    fn the_build_is_the_size_and_scale_the_hud_draws_at() {
        for (window, scale, unicode) in [
            ([320, 240], 0, false),
            ([854, 480], 1, false),
            ([1280, 720], 2, true),
            ([1920, 1080], 3, false),
            ([2560, 1440], 4, false),
            ([3840, 2160], 0, true),
            ([1279, 719], 2, false),
        ] {
            let gui = GuiSize::new(window, scale, unicode);
            let batch = HudBatch::new(
                window,
                GuiScale {
                    requested: scale,
                    unicode,
                },
            );
            let mut state = ScreenBuildState::default();
            state.sync(Screen::Inventory, true, None, gui);
            let build = state.current.as_ref().unwrap();
            assert_eq!(
                build.size,
                (batch.width, batch.height),
                "{window:?} at {scale}"
            );
            assert_eq!(build.scale, batch.scale, "{window:?} at {scale}");
            assert_eq!(build.rect, [0.0, 0.0, batch.width, batch.height]);
            assert!(matches!(build.layout, BuildLayout::Empty));
        }
    }
}
