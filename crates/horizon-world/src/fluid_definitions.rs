//! Fluid state definitions exported from the pinned game's registry.
use std::{collections::HashMap, fmt, io};

/// A missing or malformed prepared fluid-definition field.
#[derive(Debug)]
pub enum FluidDefinitionLoadError {
    /// The preparation predates the fluid state definition export.
    Missing,
    /// The exported definitions do not match their registry or contain invalid values.
    Invalid,
}
impl fmt::Display for FluidDefinitionLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "fluid_state_definitions {}: re-run horizon-prepare",
            match self {
                Self::Missing => "missing",
                Self::Invalid => "invalid",
            }
        )
    }
}
impl std::error::Error for FluidDefinitionLoadError {}

/// One registered fluid's property names, values, and default state.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FluidDefinition {
    /// Property definitions in registry order.
    pub properties: Vec<(String, Vec<String>)>,
    /// Serialized default state from the exporter.
    pub default: serde_json::Value,
}

/// Decode and validate the exported definitions against the same fluid registry.
pub fn load(
    value: &serde_json::Value,
    fluids: &[&str],
) -> io::Result<HashMap<String, FluidDefinition>> {
    let fail = |e| io::Error::new(io::ErrorKind::InvalidData, e);
    let entries = value
        .get("fluid_state_definitions")
        .ok_or_else(|| fail(FluidDefinitionLoadError::Missing))?
        .as_object()
        .ok_or_else(|| fail(FluidDefinitionLoadError::Invalid))?;
    if entries.len() != fluids.len() {
        return Err(fail(FluidDefinitionLoadError::Invalid));
    }
    let mut result = HashMap::new();
    for &id in fluids {
        let entry = entries
            .get(id)
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| fail(FluidDefinitionLoadError::Invalid))?;
        let properties = entry
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| fail(FluidDefinitionLoadError::Invalid))?;
        let mut parsed = Vec::new();
        for (name, values) in properties {
            let values = values
                .as_array()
                .ok_or_else(|| fail(FluidDefinitionLoadError::Invalid))?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| fail(FluidDefinitionLoadError::Invalid))
                })
                .collect::<io::Result<Vec<_>>>()?;
            if values.is_empty()
                || values
                    .iter()
                    .enumerate()
                    .any(|(i, v)| values[..i].contains(v))
            {
                return Err(fail(FluidDefinitionLoadError::Invalid));
            }
            parsed.push((name.clone(), values));
        }
        let default = entry
            .get("default")
            .cloned()
            .ok_or_else(|| fail(FluidDefinitionLoadError::Invalid))?;
        let state = crate::fluid::FluidState::from_json(&default.to_string())
            .map_err(|_| fail(FluidDefinitionLoadError::Invalid))?;
        if state.kind().name() != id {
            return Err(fail(FluidDefinitionLoadError::Invalid));
        }
        let defaults = default
            .get("properties")
            .and_then(serde_json::Value::as_object);
        if defaults.map_or(0, |d| d.len()) != parsed.len()
            || parsed.iter().any(|(name, values)| {
                !defaults
                    .and_then(|d| d.get(name))
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|v| values.iter().any(|x| x == v))
            })
        {
            return Err(fail(FluidDefinitionLoadError::Invalid));
        }
        result.insert(
            id.to_owned(),
            FluidDefinition {
                properties: parsed,
                default,
            },
        );
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_schema_is_typed_and_defaults_are_checked() {
        let error = load(&serde_json::json!({}), &["minecraft:water"]).unwrap_err();
        assert!(matches!(
            error
                .get_ref()
                .and_then(|e| e.downcast_ref::<FluidDefinitionLoadError>()),
            Some(FluidDefinitionLoadError::Missing)
        ));
        let mut value = serde_json::json!({"fluid_state_definitions":{"minecraft:water":{"properties":{"falling":["true","false"]},"default":{"id":"minecraft:water","properties":{"falling":"true"}}}}});
        let definitions = load(&value, &["minecraft:water"]).unwrap();
        assert_eq!(definitions["minecraft:water"].properties[0].0, "falling");
        value["fluid_state_definitions"]["minecraft:water"]["default"]["properties"]["falling"] =
            "bad".into();
        assert!(load(&value, &["minecraft:water"]).is_err());
    }
}
