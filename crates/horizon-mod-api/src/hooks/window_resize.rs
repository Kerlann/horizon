//! Window resize hook declaration.

pub trait Hooks {
    /// After a window resize or applying a changed GUI scale, fullscreen, or Unicode font setting.
    /// The dimensions are physical pixels; repeated physical resize events are suppressed.
    #[hook(
        side = "client",
        contract = "listen",
        neoforge = "WindowResizeEvent",
        cancellable = false
    )]
    fn window_resize();
}
