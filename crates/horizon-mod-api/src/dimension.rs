//! Dimensions a native mod registers: a level with its own build height and
//! generator, as [`register_dimension`](super::Registrar::register_dimension)
//! hands it over once at startup. Data packs make dimensions with
//! `data/<ns>/dimension` and `dimension_type` files; a registered one becomes
//! those same two entries, below every data pack, so a pack can replace it.
use horizon_world::worldgen::overrides::ModDimension;
use serde_json::{Value, json};

/// The generator every level has without a mod's own: vanilla's
/// `NoiseBasedChunkGenerator`.
pub(crate) const NOISE_GENERATOR: &str = "minecraft:noise";

/// Whether `c` is `#rrggbb`.
fn hex_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|d| d.is_ascii_hexdigit())
}

/// How a dimension chooses its biomes (`BiomeSource`): the level stem's `biome_source`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum BiomeSourceKind {
    /// The one `biome` of the settings everywhere (`minecraft:fixed`).
    #[default]
    Fixed,
    /// The `biomes` in a checkerboard of cells `2^biome_scale` chunks wide
    /// (`minecraft:checkerboard`).
    Checkerboard,
    /// The Overworld's or the Nether's multi-noise biomes, by `biome_preset`
    /// (`minecraft:multi_noise`).
    MultiNoise,
    /// The End's islands (`minecraft:the_end`).
    End,
}

/// What a mod's dimension is made of. The default is the Overworld's shape
/// and terrain over plains; the parts of a dimension type not named here are
/// the Overworld's.
#[derive(Clone, Debug, PartialEq)]
// exhaustive: mods build it themselves as register_dimension's settings
pub struct DimensionSettings {
    /// The lowest block's y: a multiple of 16, from -2032.
    pub min_y: i32,
    /// The build height: a multiple of 16, at least 16, reaching no higher than 2031.
    pub height: i32,
    /// How far up portals and chorus fruit reach, at most `height`.
    pub logical_height: i32,
    /// Whether the sky lights the level.
    pub has_skylight: bool,
    /// Whether the level has a bedrock ceiling, as the Nether does.
    pub has_ceiling: bool,
    /// The chunk generator's id: `minecraft:noise`, or one a mod registered
    /// with `register_chunk_generator`, which takes no noise settings.
    pub generator: String,
    /// The noise settings' id (`minecraft:overworld`, or a data pack's).
    pub noise_settings: String,
    /// The one biome the level has, with the `Fixed` biome source.
    pub biome: String,
    /// How the level chooses its biomes. Only `Fixed` suits a mod's own chunk generator, which
    /// writes terrain only.
    pub biome_source: BiomeSourceKind,
    /// The biomes a `Checkerboard` source alternates, at least one (vanilla sets no maximum).
    pub biomes: Vec<String>,
    /// A `Checkerboard`'s cell size as a power of two of chunks, 0 to 62 (vanilla's default is 2).
    pub biome_scale: u32,
    /// A `MultiNoise` source's preset: `minecraft:overworld` or `minecraft:nether`.
    pub biome_preset: String,
    /// `DimensionType.hasFixedTime`: the sky's time does not advance (the Nether's and the End's).
    pub has_fixed_time: bool,
    /// `DimensionType.hasEnderDragonFight`: the level runs the End's dragon fight.
    pub has_ender_dragon_fight: bool,
    /// `DimensionType.coordinateScale`: what a portal divides this level's coordinates by (the
    /// Nether's is 8); above 0 and at most 30,000,000.
    pub coordinate_scale: f64,
    /// `DimensionType.ambientLight`: the light every block gets, 0 to 1.
    pub ambient_light: f32,
    /// `DimensionType.infiniburn`: the block tag (`#minecraft:infiniburn_overworld`) whose blocks
    /// burn forever.
    pub infiniburn: String,
    /// `DimensionType.monsterSpawnLightTest`: the highest light a monster spawns in is drawn from
    /// this range, both ends 0 to 15.
    pub monster_spawn_light_level: (i32, i32),
    /// `DimensionType.monsterSpawnBlockLightLimit`: the block light above which monsters do not
    /// spawn, 0 to 15.
    pub monster_spawn_block_light_limit: i32,
    /// The sky drawn: `overworld`, `end` or `none` (`DimensionType.skybox`).
    pub skybox: String,
    /// How block faces are lit by direction: `default` or `nether` (`DimensionType.cardinalLightType`).
    pub cardinal_light: String,
    /// `minecraft:gameplay/water_evaporates`: water placed here boils away, as in the Nether.
    pub water_evaporates: bool,
    /// `minecraft:gameplay/fast_lava`: lava flows at the Nether's speed.
    pub fast_lava: bool,
    /// `minecraft:gameplay/respawn_anchor_works`: respawn anchors can be charged and used.
    pub respawn_anchor_works: bool,
    /// `minecraft:gameplay/nether_portal_spawns_piglin`: nether portals spawn zombified piglins.
    pub nether_portal_spawns_piglin: bool,
    /// `minecraft:gameplay/piglins_zombify`: piglins turn into zombified piglins here.
    pub piglins_zombify: bool,
    /// `minecraft:gameplay/snow_golem_melts`: snow golems take damage here.
    pub snow_golem_melts: bool,
    /// `minecraft:gameplay/can_start_raid`: a bad omen can start a raid here.
    pub can_start_raid: bool,
    /// `minecraft:gameplay/sky_light_level`: the sky light that daylight is measured against, 0 to 15.
    pub sky_light_level: f32,
    /// `minecraft:visual/sky_color` as `#rrggbb`: the sky overhead; None is the Overworld's.
    pub sky_color: Option<String>,
    /// `minecraft:visual/fog_color` as `#rrggbb`: the fog at the horizon; None is the Overworld's.
    pub fog_color: Option<String>,
    /// `minecraft:visual/cloud_height`: the clouds' height in blocks; None is the Overworld's.
    pub cloud_height: Option<f32>,
}

impl Default for DimensionSettings {
    fn default() -> Self {
        Self {
            min_y: -64,
            height: 384,
            logical_height: 384,
            has_skylight: true,
            has_ceiling: false,
            generator: NOISE_GENERATOR.into(),
            noise_settings: "minecraft:overworld".into(),
            biome: "minecraft:plains".into(),
            biome_source: BiomeSourceKind::Fixed,
            biomes: Vec::new(),
            biome_scale: 2,
            biome_preset: "minecraft:overworld".into(),
            has_fixed_time: false,
            has_ender_dragon_fight: false,
            coordinate_scale: 1.0,
            ambient_light: 0.0,
            infiniburn: "#minecraft:infiniburn_overworld".into(),
            monster_spawn_light_level: (0, 7),
            monster_spawn_block_light_limit: 0,
            skybox: "overworld".into(),
            cardinal_light: "default".into(),
            water_evaporates: false,
            fast_lava: false,
            respawn_anchor_works: false,
            nether_portal_spawns_piglin: true,
            piglins_zombify: true,
            snow_golem_melts: false,
            can_start_raid: true,
            sky_light_level: 15.0,
            sky_color: None,
            fog_color: None,
            cloud_height: None,
        }
    }
}

impl DimensionSettings {
    /// The dimension type's fields these settings set.
    fn dimension_type(&self) -> Value {
        let mut json = json!({
            "min_y": self.min_y,
            "height": self.height,
            "logical_height": self.logical_height,
            "has_skylight": self.has_skylight,
            "has_ceiling": self.has_ceiling,
            "has_fixed_time": self.has_fixed_time,
            "has_ender_dragon_fight": self.has_ender_dragon_fight,
            "coordinate_scale": self.coordinate_scale,
            "ambient_light": self.ambient_light,
            "infiniburn": self.infiniburn,
            "monster_spawn_light_level": {
                "type": "minecraft:uniform",
                "min_inclusive": self.monster_spawn_light_level.0,
                "max_inclusive": self.monster_spawn_light_level.1,
            },
            "monster_spawn_block_light_limit": self.monster_spawn_block_light_limit,
            "skybox": self.skybox,
            "cardinal_light": self.cardinal_light,
            "attributes": {
                "minecraft:gameplay/water_evaporates": self.water_evaporates,
                "minecraft:gameplay/fast_lava": self.fast_lava,
                "minecraft:gameplay/respawn_anchor_works": self.respawn_anchor_works,
                "minecraft:gameplay/nether_portal_spawns_piglin": self.nether_portal_spawns_piglin,
                "minecraft:gameplay/piglins_zombify": self.piglins_zombify,
                "minecraft:gameplay/snow_golem_melts": self.snow_golem_melts,
                "minecraft:gameplay/can_start_raid": self.can_start_raid,
                "minecraft:gameplay/sky_light_level": self.sky_light_level,
            },
        });
        let attributes = json["attributes"]
            .as_object_mut()
            .expect("attributes is an object");
        if let Some(color) = &self.sky_color {
            attributes.insert("minecraft:visual/sky_color".into(), json!(color));
        }
        if let Some(color) = &self.fog_color {
            attributes.insert("minecraft:visual/fog_color".into(), json!(color));
        }
        if let Some(height) = self.cloud_height {
            attributes.insert("minecraft:visual/cloud_height".into(), json!(height));
        }
        json
    }
    /// The level stem's `biome_source`.
    fn biome_source(&self) -> Value {
        match self.biome_source {
            BiomeSourceKind::Fixed => json!({"type": "minecraft:fixed", "biome": self.biome}),
            BiomeSourceKind::Checkerboard => json!({
                "type": "minecraft:checkerboard",
                "biomes": self.biomes,
                "scale": self.biome_scale,
            }),
            BiomeSourceKind::MultiNoise => {
                json!({"type": "minecraft:multi_noise", "preset": self.biome_preset})
            }
            BiomeSourceKind::End => json!({"type": "minecraft:the_end"}),
        }
    }
    /// What the biome source's codec refuses, if anything.
    pub(crate) fn invalid_biome_source(&self) -> Option<String> {
        let reason = match self.biome_source {
            BiomeSourceKind::Fixed => return None,
            BiomeSourceKind::Checkerboard if self.biomes.is_empty() => {
                "a checkerboard needs at least one biome"
            }
            BiomeSourceKind::Checkerboard if self.biome_scale > 62 => "biome_scale must be 0 to 62",
            BiomeSourceKind::MultiNoise
                if !matches!(
                    self.biome_preset.as_str(),
                    "minecraft:overworld" | "minecraft:nether"
                ) =>
            {
                "biome_preset must be minecraft:overworld or minecraft:nether"
            }
            _ if self.generator != NOISE_GENERATOR => {
                "only the fixed biome source suits a chunk generator of a mod's own"
            }
            _ => return None,
        };
        Some(reason.to_owned())
    }
    /// What the dimension type's codec refuses in the fields besides the build height, if
    /// anything: a value out of its range, or a name the game has no such sky or lighting for.
    pub(crate) fn invalid_field(&self) -> Option<String> {
        let light = |n: i32| (0..=15).contains(&n);
        let (low, high) = self.monster_spawn_light_level;
        let reason = if !(self.coordinate_scale > 0.0 && self.coordinate_scale <= 30_000_000.0) {
            "coordinate_scale must be above 0 and at most 30000000"
        } else if !(0.0..=1.0).contains(&self.ambient_light) {
            "ambient_light must be 0 to 1"
        } else if !(0.0..=15.0).contains(&self.sky_light_level) {
            "sky_light_level must be 0 to 15"
        } else if !(light(low) && light(high) && low <= high) {
            "monster_spawn_light_level must be a range inside 0 to 15"
        } else if !light(self.monster_spawn_block_light_limit) {
            "monster_spawn_block_light_limit must be 0 to 15"
        } else if !matches!(self.skybox.as_str(), "overworld" | "end" | "none") {
            "skybox must be overworld, end or none"
        } else if !matches!(self.cardinal_light.as_str(), "default" | "nether") {
            "cardinal_light must be default or nether"
        } else if !self.infiniburn.starts_with('#') || self.infiniburn.len() < 2 {
            "infiniburn must be a block tag, such as #minecraft:infiniburn_overworld"
        } else if !self
            .sky_color
            .iter()
            .chain(&self.fog_color)
            .all(|c| hex_color(c))
        {
            "sky_color and fog_color must be #rrggbb"
        } else if self.cloud_height.is_some_and(|h| !h.is_finite()) {
            "cloud_height must be a number"
        } else {
            return None;
        };
        Some(reason.to_owned())
    }
    /// Whether the dimension type's codec accepts the build height.
    pub(crate) fn valid_shape(&self) -> bool {
        horizon_world::worldgen::overrides::LevelHeight::of_json(&self.dimension_type()).is_some()
    }
}

/// A level's dimension type as the game uses it (`Level.dimensionType()` and the gameplay attributes
/// of its environment), as `level.dimension_type(host)` reads it: the fields of
/// [`DimensionSettings`] that apply, plus the type's id.
#[derive(Clone, Debug, PartialEq)]
// exhaustive: the game builds it with a literal; mods only read it
pub struct LevelDimensionType {
    /// The dimension type's id (`minecraft:the_nether`, or a mod's or pack's).
    pub id: String,
    /// The lowest buildable block Y.
    pub min_y: i32,
    /// The build height in blocks.
    pub height: i32,
    /// How far up portals and chorus fruit reach.
    pub logical_height: i32,
    /// Whether the sky lights the level.
    pub has_skylight: bool,
    /// Whether the level has a bedrock ceiling.
    pub has_ceiling: bool,
    /// Whether the sky's time does not advance.
    pub has_fixed_time: bool,
    /// Whether the level runs the End's dragon fight.
    pub has_ender_dragon_fight: bool,
    /// What a portal divides this level's coordinates by.
    pub coordinate_scale: f64,
    /// The light every block gets, 0 to 1.
    pub ambient_light: f32,
    /// The block tag whose blocks burn forever.
    pub infiniburn: String,
    /// The range monsters' spawn light is drawn from, when it is a constant or a uniform range.
    pub monster_spawn_light_level: Option<(i32, i32)>,
    /// The block light above which monsters do not spawn.
    pub monster_spawn_block_light_limit: i32,
    /// `overworld`, `end` or `none`.
    pub skybox: String,
    /// `default` or `nether`.
    pub cardinal_light: String,
    /// Whether water placed here boils away.
    pub water_evaporates: bool,
    /// Whether lava flows at the Nether's speed.
    pub fast_lava: bool,
    /// Whether respawn anchors can be used.
    pub respawn_anchor_works: bool,
    /// Whether nether portals spawn zombified piglins.
    pub nether_portal_spawns_piglin: bool,
    /// Whether piglins turn into zombified piglins here.
    pub piglins_zombify: bool,
    /// Whether snow golems take damage here.
    pub snow_golem_melts: bool,
    /// Whether a bad omen can start a raid here.
    pub can_start_raid: bool,
    /// The sky light that daylight is measured against, 0 to 15.
    pub sky_light_level: f32,
}

/// A registered dimension: its namespaced id (`modid:pocket`), the mod that
/// registered it and its settings.
pub(crate) struct DimensionInfo {
    pub(crate) mod_id: &'static str,
    pub(crate) id: Box<str>,
    pub(crate) settings: DimensionSettings,
}

impl DimensionInfo {
    /// The level stem and dimension type it stands for.
    fn entries(&self) -> ModDimension {
        let settings = &self.settings;
        let stem = json!({
            "type": &*self.id,
            "generator": {
                "type": settings.generator,
                "settings": settings.noise_settings,
                "biome_source": settings.biome_source(),
            },
        });
        ModDimension {
            id: self.id.to_string(),
            stem: stem.to_string(),
            dimension_type: settings.dimension_type().to_string(),
        }
    }
}

/// The dimensions the mods registered at startup, as the engine reads them.
/// Empty before the parts are installed.
pub fn installed_dimensions() -> Vec<ModDimension> {
    super::parts::installed_dimension_list()
        .iter()
        .map(DimensionInfo::entries)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_settings_are_accepted_and_name_the_overworlds_type() {
        let settings = DimensionSettings::default();
        assert!(settings.valid_shape());
        assert_eq!(settings.invalid_field(), None);
        let json = settings.dimension_type();
        assert_eq!(json["coordinate_scale"], 1.0);
        assert_eq!(json["infiniburn"], "#minecraft:infiniburn_overworld");
        assert_eq!(json["monster_spawn_light_level"]["max_inclusive"], 7);
        assert_eq!(
            json["attributes"]["minecraft:gameplay/piglins_zombify"],
            true
        );
        assert_eq!(json["attributes"]["minecraft:gameplay/fast_lava"], false);
    }

    #[test]
    fn a_field_out_of_range_is_refused_by_name() {
        let refused = |edit: fn(&mut DimensionSettings)| {
            let mut settings = DimensionSettings::default();
            edit(&mut settings);
            settings.invalid_field().unwrap_or_default()
        };
        assert!(refused(|s| s.coordinate_scale = 0.0).contains("coordinate_scale"));
        assert!(refused(|s| s.coordinate_scale = f64::NAN).contains("coordinate_scale"));
        assert!(refused(|s| s.ambient_light = 1.5).contains("ambient_light"));
        assert!(refused(|s| s.sky_light_level = 16.0).contains("sky_light_level"));
        assert!(
            refused(|s| s.monster_spawn_light_level = (8, 7)).contains("monster_spawn_light_level")
        );
        assert!(refused(|s| s.monster_spawn_block_light_limit = 16).contains("block_light_limit"));
        assert!(refused(|s| s.skybox = "moon".into()).contains("skybox"));
        assert!(refused(|s| s.cardinal_light = "up".into()).contains("cardinal_light"));
        assert!(
            refused(|s| s.infiniburn = "minecraft:infiniburn_end".into()).contains("infiniburn")
        );
    }
}

#[cfg(test)]
mod biome_source_tests {
    use super::*;

    #[test]
    fn each_source_names_the_stem_the_data_pack_codec_reads() {
        let mut settings = DimensionSettings::default();
        assert_eq!(
            settings.biome_source(),
            json!({"type": "minecraft:fixed", "biome": "minecraft:plains"})
        );
        settings.biome_source = BiomeSourceKind::Checkerboard;
        settings.biomes = vec!["minecraft:desert".into(), "minecraft:plains".into()];
        settings.biome_scale = 3;
        assert_eq!(
            settings.biome_source(),
            json!({
                "type": "minecraft:checkerboard",
                "biomes": ["minecraft:desert", "minecraft:plains"],
                "scale": 3
            })
        );
        settings.biome_source = BiomeSourceKind::MultiNoise;
        settings.biome_preset = "minecraft:nether".into();
        assert_eq!(
            settings.biome_source(),
            json!({"type": "minecraft:multi_noise", "preset": "minecraft:nether"})
        );
        settings.biome_source = BiomeSourceKind::End;
        assert_eq!(
            settings.biome_source(),
            json!({"type": "minecraft:the_end"})
        );
    }

    #[test]
    fn a_source_the_codec_refuses_is_refused_by_name() {
        let refused = |edit: fn(&mut DimensionSettings)| {
            let mut settings = DimensionSettings::default();
            edit(&mut settings);
            settings.invalid_biome_source().unwrap_or_default()
        };
        assert_eq!(refused(|_| {}), "");
        // vanilla's codec takes a holder set of any size, so 65 biomes are accepted
        assert_eq!(
            refused(|s| {
                s.biome_source = BiomeSourceKind::Checkerboard;
                s.biomes = (0..65).map(|n| format!("test:biome_{n}")).collect();
            }),
            ""
        );
        assert!(
            refused(|s| s.biome_source = BiomeSourceKind::Checkerboard).contains("checkerboard")
        );
        assert!(
            refused(|s| {
                s.biome_source = BiomeSourceKind::Checkerboard;
                s.biomes = vec!["minecraft:plains".into()];
                s.biome_scale = 63;
            })
            .contains("biome_scale")
        );
        assert!(
            refused(|s| {
                s.biome_source = BiomeSourceKind::MultiNoise;
                s.biome_preset = "minecraft:end".into();
            })
            .contains("biome_preset")
        );
        assert!(
            refused(|s| {
                s.biome_source = BiomeSourceKind::End;
                s.generator = "mymod:void".into();
            })
            .contains("fixed biome source")
        );
        assert_eq!(
            refused(|s| {
                s.biome_source = BiomeSourceKind::MultiNoise;
            }),
            ""
        );
    }
}

#[cfg(test)]
mod visual_tests {
    use super::*;

    #[test]
    fn the_sky_fog_and_clouds_are_written_only_when_set() {
        let plain = DimensionSettings::default().dimension_type();
        assert!(
            plain["attributes"]
                .get("minecraft:visual/sky_color")
                .is_none()
        );
        let settings = DimensionSettings {
            sky_color: Some("#112233".into()),
            fog_color: Some("#445566".into()),
            cloud_height: Some(192.0),
            ..Default::default()
        };
        assert_eq!(settings.invalid_field(), None);
        let attributes = &settings.dimension_type()["attributes"];
        assert_eq!(attributes["minecraft:visual/sky_color"], "#112233");
        assert_eq!(attributes["minecraft:visual/fog_color"], "#445566");
        assert_eq!(attributes["minecraft:visual/cloud_height"], 192.0);
    }

    #[test]
    fn a_colour_that_is_not_rrggbb_is_refused() {
        for bad in ["112233", "#12345", "#12345g", "#1234567"] {
            let settings = DimensionSettings {
                sky_color: Some(bad.into()),
                ..Default::default()
            };
            assert!(
                settings.invalid_field().unwrap().contains("#rrggbb"),
                "{bad}"
            );
        }
    }
}
