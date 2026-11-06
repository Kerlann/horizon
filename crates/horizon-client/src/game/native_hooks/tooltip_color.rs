use super::*;

/// The hook points. Each does nothing in Horizon.
impl Game<'_> {
    /// Change a tooltip box's background and border gradient colors, as ARGB.
    /// Stack is None for tooltips not associated with an item.
    pub(in crate::game) fn native_tooltip_color(
        &self,
        stack: Option<&ItemStack>,
        background: &mut u32,
        border_top: &mut u32,
        border_bottom: &mut u32,
    ) {
        self.fire_tooltip_color(stack, background, border_top, border_bottom);
    }
}
