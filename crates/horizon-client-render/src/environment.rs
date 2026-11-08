//! A dimension's environment attributes (`EnvironmentAttributeSystem.addDefaultLayers`): the
//! dimension type's attributes, the biome layer, the dimension's timelines and, where it can
//! have weather, the rain and thunder layers, sampled into the sky state the renderer draws.
use crate::Result;
use horizon_assets::resource_source::ResourceSource;
use horizon_world::environment_attributes::{AttributeMap, Value, attribute};
use horizon_world::environment_system::{BiomeBlend, Builder, Conditions, System};
use horizon_world::environment_timeline::Timeline;
use serde_json::Value as Json;
use std::collections::HashMap;
use std::sync::{LazyLock, OnceLock, RwLock};

pub struct Environment {
    system: System,
    /// What it was built from, so a level can have the same environment with its own attributes.
    source: ResourceSource,
    dimension: String,
}

/// The attributes a mod or a data pack set in the dimension types of the levels it added, by level
/// id, and the environment each makes over the Overworld's the first time it is asked for
/// (`None` when its attributes are refused, which falls back to the Overworld's).
type LevelAttributes = HashMap<String, &'static (Json, OnceLock<Option<Environment>>)>;
static LEVELS: LazyLock<RwLock<LevelAttributes>> = LazyLock::new(Default::default);

#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Sky {
    pub sky_color: [f32; 3],
    pub fog_color: [f32; 3],
    pub sky_light_factor: f32,
    pub sky_light_color: [f32; 3],
    pub block_light_tint: [f32; 3],
    pub ambient_light_color: [f32; 3],
    pub sun_angle: f32,
    pub moon_angle: f32,
    pub star_angle: f32,
    pub star_brightness: f32,
    pub sunrise_color: [f32; 4],
    pub cloud_color: [f32; 4],
    /// `gameplay/sky_light_level`, the server-side sky darkening.
    pub sky_light_level: f32,
    /// Environmental fog (`visual/fog_start_distance` / `fog_end_distance`).
    pub fog_start: f32,
    pub fog_end: f32,
    /// `visual/cloud_fog_end_distance`.
    pub cloud_fog_end: f32,
    /// `visual/cloud_height`.
    pub cloud_height: f32,
}

/// Where and when the sky is sampled.
#[derive(Clone, Copy, Default)]
pub struct Sample<'a> {
    /// The day clock's total ticks.
    pub day_time: i64,
    /// `Level.getRainLevel` and `getThunderLevel`, 0 where the dimension has no weather.
    pub rain: f32,
    pub thunder: f32,
    /// The biomes around the camera (`EnvironmentAttributeProbe`'s Gaussian mix).
    pub blend: Option<&'a BiomeBlend<'a>>,
    /// The biome at the camera, for attributes that don't blend.
    pub biome: Option<&'a AttributeMap>,
}

/// The timeline ids a dimension type's `timelines` names: an id, a `#tag` or a list,
/// tags resolved through `tags/timeline/` (their entries may be `{"id", "required"}`).
fn timeline_ids(root: &ResourceSource, json: &Json, out: &mut Vec<String>, depth: u32) {
    let entry = |e: &Json| {
        e.as_str()
            .or_else(|| e.get("id").and_then(Json::as_str))
            .map(str::to_owned)
    };
    let items: Vec<String> = match json {
        Json::Array(list) => list.iter().filter_map(entry).collect(),
        other => entry(other).into_iter().collect(),
    };
    for item in items {
        if let Some(tag) = item.strip_prefix('#') {
            let (namespace, path) = tag.split_once(':').unwrap_or(("minecraft", tag));
            let file = format!("data/{namespace}/tags/timeline/{path}.json");
            if depth < 8
                && let Ok(tag) = root
                    .read(&file)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| {
                        serde_json::from_slice::<Json>(&bytes).map_err(|e| e.to_string())
                    })
            {
                timeline_ids(root, &tag["values"], out, depth + 1);
            }
        } else if !out.contains(&item) {
            out.push(item);
        }
    }
}

impl Environment {
    pub fn load(root: &ResourceSource) -> Result<Self> {
        Self::load_dimension(root, "overworld")
    }

    /// `EnvironmentAttributeSystem.addDefaultLayers` for a dimension type: its attributes, a
    /// biome layer for every attribute some biome sets, its timelines (in tag order), and the
    /// weather where `Level.canHaveWeather` (sky light, no ceiling, not the End).
    pub fn load_dimension(root: &ResourceSource, dimension: &str) -> Result<Self> {
        Self::load_dimension_with(root, dimension, &Json::Null)
    }

    /// [`Self::load_dimension`] with the attributes in `extra` (an `attributes` object) over the
    /// dimension type's own, key by key.
    pub fn load_dimension_with(
        root: &ResourceSource,
        dimension: &str,
        extra: &Json,
    ) -> Result<Self> {
        let mut json: Json = serde_json::from_slice(
            &root.read(&format!("data/minecraft/dimension_type/{dimension}.json"))?,
        )?;
        if let Some(extra) = extra.as_object() {
            if !json["attributes"].is_object() {
                json["attributes"] = Json::Object(Default::default());
            }
            if let Some(own) = json["attributes"].as_object_mut() {
                own.extend(extra.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
        }
        let mut builder = Builder::default();
        builder.constant_layer(&AttributeMap::parse(&json["attributes"])?);
        // Without biome data (a bare dimension type), there is no biome layer.
        let mut biomes = Vec::new();
        for file in root.list("data/minecraft/worldgen/biome/") {
            if !file.ends_with(".json") {
                continue;
            }
            let biome: Json = serde_json::from_slice(&root.read(&file)?)?;
            match AttributeMap::parse(&biome["attributes"]) {
                Ok(map) => biomes.push(map),
                Err(e) => log::warn!("biome {file}: {e}"),
            }
        }
        builder.biome_layer(&biomes);
        let mut ids = Vec::new();
        timeline_ids(root, &json["timelines"], &mut ids, 0);
        for id in ids {
            let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", &id));
            let file = format!("data/{namespace}/timeline/{path}.json");
            match root
                .read(&file)
                .map_err(|e| e.to_string())
                .and_then(|bytes| serde_json::from_slice::<Json>(&bytes).map_err(|e| e.to_string()))
                .map_err(|e| e.to_string())
                .and_then(|t| Timeline::parse(&t))
            {
                Ok(timeline) => {
                    builder.timeline_layer(&timeline);
                }
                Err(e) => log::warn!("timeline {id}: {e}"),
            }
        }
        let weather = json["has_skylight"].as_bool() == Some(true)
            && json["has_ceiling"].as_bool() != Some(true)
            && dimension != "the_end";
        if weather {
            builder.weather_layers();
        }
        Ok(Self {
            system: builder.build(),
            source: root.clone(),
            dimension: dimension.to_owned(),
        })
    }

    /// Replaces the levels' own attributes (a dimension type's `attributes` object), at a world's
    /// load. The table is leaked: one small entry per level and world load.
    pub fn set_level_attributes(levels: Vec<(String, Json)>) {
        let table = levels
            .into_iter()
            .map(|(id, attributes)| {
                let entry = Box::leak(Box::new((attributes, OnceLock::new())));
                (id, &*entry)
            })
            .collect();
        *LEVELS.write().unwrap_or_else(|e| e.into_inner()) = table;
    }

    /// `base` with the attributes a level's dimension type set over it, for a level a mod or a
    /// data pack added; `base` itself for any other, or when the attributes are refused.
    pub fn for_level<'a>(level: &str, base: &'a Environment) -> &'a Environment {
        let entry = LEVELS
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(level)
            .copied();
        let Some((attributes, made)) = entry else {
            return base;
        };
        made.get_or_init(|| {
            Self::load_dimension_with(&base.source, &base.dimension, attributes)
                .map_err(|e| log::warn!("dimension {level}: attributes refused: {e}"))
                .ok()
        })
        .as_ref()
        .unwrap_or(base)
    }

    /// `EnvironmentAttributeSystem.getValue` of one attribute, or its dimension value without
    /// a biome.
    pub fn value(&self, id: &str, at: &Sample) -> Option<Value> {
        let attribute = attribute(id)?;
        let biome = |_full_resolution: bool| at.biome;
        let clock = |_clock: &str| at.day_time;
        let conditions = Conditions {
            rain: at.rain,
            thunder: at.thunder,
            blend: at.blend,
            biome: &biome,
            clock: &clock,
        };
        match (at.blend, at.biome) {
            (None, None) => self.system.dimension_value(attribute, &conditions),
            _ => self.system.value(attribute, &conditions),
        }
    }

    /// The sky state the renderer draws.
    pub fn sample(&self, at: &Sample) -> Sky {
        let float = |id: &str| match self.value(id, at) {
            Some(Value::Float(f)) => f,
            _ => 0.0,
        };
        let rgb = |id: &str| match self.value(id, at) {
            Some(Value::Rgb(c)) => c,
            _ => [0.0; 3],
        };
        let argb = |id: &str| match self.value(id, at) {
            Some(Value::Argb(c)) => c,
            _ => [0.0; 4],
        };
        Sky {
            sky_color: rgb("visual/sky_color"),
            fog_color: rgb("visual/fog_color"),
            sky_light_factor: float("visual/sky_light_factor"),
            sky_light_color: rgb("visual/sky_light_color"),
            block_light_tint: rgb("visual/block_light_tint"),
            ambient_light_color: rgb("visual/ambient_light_color"),
            sun_angle: float("visual/sun_angle"),
            moon_angle: float("visual/moon_angle"),
            star_angle: float("visual/star_angle"),
            star_brightness: float("visual/star_brightness"),
            sunrise_color: argb("visual/sunrise_sunset_color"),
            cloud_color: argb("visual/cloud_color"),
            sky_light_level: float("gameplay/sky_light_level") / 15.0,
            fog_start: float("visual/fog_start_distance"),
            fog_end: float("visual/fog_end_distance"),
            cloud_fog_end: float("visual/cloud_fog_end_distance"),
            cloud_height: float("visual/cloud_height"),
        }
    }
}

/// `Level.updateSkyBrightness`'s darkening: from the sky light level, a dimension value.
pub fn sky_darken(environment: &Environment, day_time: i64, rain: f32, thunder: f32) -> i32 {
    let level = match environment.value(
        "gameplay/sky_light_level",
        &Sample {
            day_time,
            rain,
            thunder,
            ..Sample::default()
        },
    ) {
        Some(Value::Float(level)) => level,
        _ => 0.0,
    };
    // Truncate the raw light level without a lossy normalize / denormalize round trip.
    (15.0 - level) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The level table is global, so the tests that set it take turns.
    static LEVEL_TABLE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn resolved_dimension_data_and_raw_darkening_need_no_files() {
        use horizon_assets::resource_source::Resource;
        for (level, expected) in [(14.5, 0), (13.5, 1), (4.0, 11)] {
            let source = ResourceSource::indexed(std::collections::BTreeMap::from([(
                "data/minecraft/dimension_type/overworld.json".into(),
                Resource::bytes(
                    format!(r#"{{"attributes":{{"minecraft:gameplay/sky_light_level":{level}}}}}"#)
                        .into_bytes(),
                ),
            )]));
            let environment = Environment::load(&source).unwrap();
            assert_eq!(sky_darken(&environment, 0, 0.0, 0.0), expected);
            assert!(Environment::load_dimension(&source, "missing").is_err());
        }
    }

    fn overworld() -> Option<Environment> {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client/26.3");
        root.join("data/minecraft/timeline/day.json")
            .exists()
            .then(|| Environment::load(&ResourceSource::directory(root)).unwrap())
    }

    #[test]
    fn dark_outside_follows_time_and_weather_levels() {
        let Some(env) = overworld() else {
            return;
        };
        let dark = |time, rain, thunder| sky_darken(&env, time, rain, thunder) >= 4;
        assert!(!dark(6000, 0.0, 0.0), "clear noon");
        assert!(!dark(6000, 1.0, 0.0), "rainy noon");
        assert!(dark(6000, 1.0, 1.0), "thunderstorm at noon");
        assert!(dark(18000, 0.0, 0.0), "clear midnight");
        assert!(!dark(1000, 0.0, 0.0), "clear morning");
    }

    /// A level's attributes go over the Overworld's one by one; only a registered level gets its
    /// own environment, and the table clears.
    #[test]
    fn only_a_registered_level_gets_its_own_environment() {
        let _turn = LEVEL_TABLE.lock().unwrap_or_else(|e| e.into_inner());
        use horizon_assets::resource_source::Resource;
        let source = ResourceSource::indexed(std::collections::BTreeMap::from([(
            "data/minecraft/dimension_type/overworld.json".into(),
            Resource::bytes(
                br#"{"attributes":{"minecraft:gameplay/sky_light_level":15.0}}"#.to_vec(),
            ),
        )]));
        let base = Environment::load(&source).unwrap();
        let id = "test:env_pocket";
        let darken = |e: &Environment| sky_darken(e, 0, 0.0, 0.0);
        assert!(std::ptr::eq(Environment::for_level(id, &base), &base));
        Environment::set_level_attributes(vec![(
            id.to_owned(),
            serde_json::json!({"minecraft:gameplay/sky_light_level": 4.0}),
        )]);
        let own = Environment::for_level(id, &base);
        assert!(!std::ptr::eq(own, &base));
        assert_eq!((darken(&base), darken(own)), (0, 11));
        assert!(std::ptr::eq(
            Environment::for_level("test:other", &base),
            &base
        ));
        Environment::set_level_attributes(Vec::new());
        assert!(std::ptr::eq(Environment::for_level(id, &base), &base));
    }

    /// Attributes the game refuses leave the level on the Overworld's environment.
    #[test]
    fn refused_attributes_fall_back_to_the_base() {
        let _turn = LEVEL_TABLE.lock().unwrap_or_else(|e| e.into_inner());
        use horizon_assets::resource_source::Resource;
        let source = ResourceSource::indexed(std::collections::BTreeMap::from([(
            "data/minecraft/dimension_type/overworld.json".into(),
            Resource::bytes(br#"{"attributes":{}}"#.to_vec()),
        )]));
        let base = Environment::load(&source).unwrap();
        Environment::set_level_attributes(vec![(
            "test:bad_env".to_owned(),
            serde_json::json!({"minecraft:visual/not_an_attribute": 1}),
        )]);
        assert!(std::ptr::eq(
            Environment::for_level("test:bad_env", &base),
            &base
        ));
        Environment::set_level_attributes(Vec::new());
    }
}
