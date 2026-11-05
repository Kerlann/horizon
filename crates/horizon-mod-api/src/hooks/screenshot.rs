//! The `screenshot` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// An F2 screenshot has been read back, before either PNG is written
    /// (`ScreenshotEvent`). Runs on the client thread with a `ClientHost`;
    /// no pixels cross the API. Change `file_name`, a relative path inside
    /// the player's screenshots folder, or set `message` to replace the
    /// success chat line. `None` uses the normal translated, clickable line
    /// for the final file name. Return `Flow::Cancel` to write no files and
    /// show `message`, or a default cancellation line when it is `None`.
    /// Invalid paths (including symlinks) report screenshot failure;
    /// parent folders are created. HDR copies follow the final name with
    /// `-hdr.png`. Agent captures and world icons do not fire this hook.
    #[hook(
        side = "client",
        contract = "cancel",
        out = "cancel",
        neoforge = "ScreenshotEvent",
        cancellable = true
    )]
    fn screenshot();
}
