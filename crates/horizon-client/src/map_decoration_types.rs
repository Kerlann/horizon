//! The map decoration types native mods register (`map_decoration_types`, NeoForge's
//! `Registries.MAP_DECORATION_TYPE`), frozen once at start. A type's id and sprite live for the
//! rest of the process, as the vanilla table's do, so a decoration holds them as `&'static str`.

use mod_api::{Identifier, MapDecorationType};
use std::collections::HashMap;
use std::sync::OnceLock;

/// What the game keeps of a registered type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Entry {
    id: &'static str,
    sprite: &'static str,
    show_on_item_frame: bool,
}

/// The registered types by id.
#[derive(Debug, Default)]
pub struct MapDecorationTypes {
    by_id: HashMap<&'static str, Entry>,
    by_sprite: HashMap<&'static str, Entry>,
}

/// `MapDecorationTypes` Horizon draws: (type, sprite).
pub const VANILLA_TYPES: [(&str, &str); 40] = [
    ("player", "map/decorations/player"),
    ("frame", "map/decorations/frame"),
    ("red_marker", "map/decorations/red_marker"),
    ("blue_marker", "map/decorations/blue_marker"),
    ("target_x", "map/decorations/target_x"),
    ("target_point", "map/decorations/target_point"),
    ("player_off_map", "map/decorations/player_off_map"),
    ("player_off_limits", "map/decorations/player_off_limits"),
    ("mansion", "map/decorations/woodland_mansion"),
    ("monument", "map/decorations/ocean_monument"),
    ("banner_white", "map/decorations/white_banner"),
    ("banner_orange", "map/decorations/orange_banner"),
    ("banner_magenta", "map/decorations/magenta_banner"),
    ("banner_light_blue", "map/decorations/light_blue_banner"),
    ("banner_yellow", "map/decorations/yellow_banner"),
    ("banner_lime", "map/decorations/lime_banner"),
    ("banner_pink", "map/decorations/pink_banner"),
    ("banner_gray", "map/decorations/gray_banner"),
    ("banner_light_gray", "map/decorations/light_gray_banner"),
    ("banner_cyan", "map/decorations/cyan_banner"),
    ("banner_purple", "map/decorations/purple_banner"),
    ("banner_blue", "map/decorations/blue_banner"),
    ("banner_brown", "map/decorations/brown_banner"),
    ("banner_green", "map/decorations/green_banner"),
    ("banner_red", "map/decorations/red_banner"),
    ("banner_black", "map/decorations/black_banner"),
    ("red_x", "map/decorations/red_x"),
    ("village_desert", "map/decorations/desert_village"),
    ("village_plains", "map/decorations/plains_village"),
    ("village_savanna", "map/decorations/savanna_village"),
    ("village_snowy", "map/decorations/snowy_village"),
    ("village_taiga", "map/decorations/taiga_village"),
    ("jungle_temple", "map/decorations/jungle_temple"),
    ("swamp_hut", "map/decorations/swamp_hut"),
    ("trial_chambers", "map/decorations/trial_chambers"),
    ("abandoned_camp", "map/decorations/abandoned_camp"),
    ("ancient_city", "map/decorations/ancient_city"),
    ("desert_pyramid", "map/decorations/desert_pyramid"),
    ("mineshaft", "map/decorations/mineshaft"),
    ("ocean_ruin_warm", "map/decorations/warm_ocean_ruins"),
];

static TYPES: OnceLock<MapDecorationTypes> = OnceLock::new();

/// Freezes the types native mods registered, once at start.
pub fn register(types: Vec<MapDecorationType>) {
    if TYPES.get().is_some() {
        log::warn!("Map decoration types were registered already; these are left out");
        return;
    }
    if TYPES.set(MapDecorationTypes::place(types)).is_err() {
        log::warn!("Map decoration types were registered already; these are left out");
    }
}

/// The registered types: none until `register` runs.
pub fn registered() -> &'static MapDecorationTypes {
    static NONE: OnceLock<MapDecorationTypes> = OnceLock::new();
    TYPES
        .get()
        .unwrap_or_else(|| NONE.get_or_init(MapDecorationTypes::default))
}

impl MapDecorationTypes {
    /// Registers each type in push order. An id that is not `<namespace>:<path>` in a mod's
    /// namespace, is registered already, or has an empty sprite or one used by vanilla or another mod is logged and
    /// left out, where NeoForge's registry throws.
    pub fn place(registered: Vec<MapDecorationType>) -> Self {
        let mut types = Self::default();
        for MapDecorationType {
            id,
            sprite,
            show_on_item_frame,
        } in registered
        {
            let own =
                Identifier::parse_namespaced(&id).is_some_and(|id| id.namespace() != "minecraft");
            if !own {
                log::warn!(
                    "Map decoration type {id:?} is not <namespace>:<path> in a mod's namespace"
                );
            } else if sprite.is_empty() {
                log::warn!("Map decoration type {id} has no sprite; it is left out");
            } else if types.by_id.contains_key(id.as_str()) {
                log::warn!("Map decoration type {id} is registered twice; the first is kept");
            } else if VANILLA_TYPES.iter().any(|(_, vanilla)| *vanilla == sprite)
                || types.by_sprite.contains_key(sprite.as_str())
            {
                log::warn!(
                    "Map decoration type {id} reuses the sprite {sprite} of another; it is left out"
                );
            } else {
                let entry = Entry {
                    id: Box::leak(id.into_boxed_str()),
                    sprite: Box::leak(sprite.into_boxed_str()),
                    show_on_item_frame,
                };
                types.by_id.insert(entry.id, entry);
                types.by_sprite.insert(entry.sprite, entry);
            }
        }
        types
    }

    /// The sprite of the type `id` (`mymod:treasure`).
    pub fn sprite(&self, id: &str) -> Option<&'static str> {
        self.by_id.get(id).map(|e| e.sprite)
    }

    /// The id of the type drawn with `sprite`.
    pub fn id_of(&self, sprite: &str) -> Option<&'static str> {
        self.by_sprite.get(sprite).map(|e| e.id)
    }

    /// `MapDecorationType.showOnItemFrame` of the type drawn with `sprite`.
    pub fn shown_on_frame(&self, sprite: &str) -> Option<bool> {
        self.by_sprite.get(sprite).map(|e| e.show_on_item_frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(id: &str, sprite: &str, frame: bool) -> MapDecorationType {
        MapDecorationType {
            id: id.into(),
            sprite: sprite.into(),
            show_on_item_frame: frame,
        }
    }

    #[test]
    fn a_mod_type_is_found_by_id_and_by_sprite() {
        let types = MapDecorationTypes::place(vec![
            declared("mymod:treasure", "mymod:gui/treasure", true),
            declared("mymod:camp", "mymod:gui/camp", false),
        ]);
        assert_eq!(types.sprite("mymod:treasure"), Some("mymod:gui/treasure"));
        assert_eq!(types.id_of("mymod:gui/camp"), Some("mymod:camp"));
        assert_eq!(types.shown_on_frame("mymod:gui/treasure"), Some(true));
        assert_eq!(types.shown_on_frame("mymod:gui/camp"), Some(false));
        assert_eq!(types.sprite("mymod:missing"), None);
        assert_eq!(types.shown_on_frame("map/decorations/player"), None);
    }

    #[test]
    fn bad_and_duplicate_types_are_left_out_and_the_first_wins() {
        let types = MapDecorationTypes::place(vec![
            declared("minecraft:red_marker", "mymod:gui/a", true),
            declared("treasure", "mymod:gui/b", true),
            declared("mymod:empty", "", true),
            declared("mymod:one", "mymod:gui/one", true),
            declared("mymod:one", "mymod:gui/two", false),
            declared("mymod:other", "mymod:gui/one", false),
        ]);
        assert_eq!(types.by_id.len(), 1);
        assert_eq!(types.sprite("mymod:one"), Some("mymod:gui/one"));
        assert_eq!(types.shown_on_frame("mymod:gui/one"), Some(true));
    }

    #[test]
    fn every_vanilla_sprite_is_refused() {
        for (name, sprite) in VANILLA_TYPES {
            let id = format!("mymod:{name}");
            let types = MapDecorationTypes::place(vec![declared(&id, sprite, true)]);
            assert_eq!(types.sprite(&id), None, "{sprite}");
            assert_eq!(types.id_of(sprite), None, "{sprite}");
            assert_eq!(types.shown_on_frame(sprite), None, "{sprite}");
        }
    }

    #[test]
    fn with_no_mod_types_nothing_is_found() {
        let types = MapDecorationTypes::default();
        assert_eq!(types.sprite("mymod:treasure"), None);
        assert_eq!(types.id_of("mymod:gui/treasure"), None);
    }
}
