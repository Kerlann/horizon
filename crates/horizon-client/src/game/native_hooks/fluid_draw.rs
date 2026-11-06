use super::*;

/// Helpers for native mods, kept as stable as the hook points.
// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// Fluid `fluid` (`minecraft:water`, `mymod:oil`) drawn as a square with its top left at `at`, `size`
    /// GUI pixels wide (16 in a slot): the sprite its look names (`data/<mod>/horizon/fluid/<name>.json`)
    /// tinted with its colour, else a flat square of the colour, which vanilla water and lava have. What a
    /// recipe viewer draws for a fluid stack; the slot and the amount are the mod's.
    pub(in crate::game) fn native_fluid(
        &self,
        batch: &mut HudBatch,
        fluid: &str,
        at: [f32; 2],
        size: f32,
    ) {
        let look = horizon_client::mod_content::fluid_look(fluid);
        let tint = crate::game::native_panel_recipes::fluid_tint(
            fluid,
            look.as_ref().and_then(|l| l.tint),
        );
        let dst = [at[0], at[1], size, size];
        let sprite = look
            .and_then(|l| l.sprite)
            .and_then(|s| self.native_sprite_size(batch, &s).map(|size| (s, size)));
        if let Some((name, px)) = sprite {
            self.native_sprite(batch, &name, [0.0, 0.0, px[0], px[1]], dst, Some(tint));
        } else {
            let [a, r, g, b] = tint.to_be_bytes();
            batch.fill(
                &self.gui.atlas,
                dst[0],
                dst[1],
                dst[2],
                dst[3],
                rgba(r, g, b, a),
            );
        }
    }
    /// Fluid `fluid`'s name as the lang names it (`fluid_type.<mod>.<name>`, vanilla's by its block),
    /// else its path in words.
    pub(in crate::game) fn native_fluid_name(&self, fluid: &str) -> String {
        crate::game::native_panel_recipes::fluid_name(fluid)
    }
}
