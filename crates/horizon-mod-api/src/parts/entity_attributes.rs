use super::*;
/// An entity type's initial attribute set, declared in code (NeoForge's
/// `EntityAttributeCreationEvent`): the type's id (`mymod:ghoul`), the
/// vanilla type its content file copies (`minecraft:zombie`, whose row the
/// set starts from, as `Zombie.createAttributes` builds on the template's)
/// and each attribute's base value that differs from the template's. The
/// game resolves the set onto the type's attribute row once, at load.
#[derive(Clone, Debug, PartialEq)]
// exhaustive: only registration builds it; the game reads it
pub struct EntityAttributeDeclaration {
    /// The entity type the set belongs to.
    pub id: String,
    /// The template the type copies; its row is the set's start.
    pub template: String,
    /// Each attribute's id (`minecraft:max_health`) and its base.
    pub attributes: Vec<(String, f64)>,
}

/// The entity attribute sets mods declared.
pub fn entity_attribute_declarations() -> &'static [EntityAttributeDeclaration] {
    installed().map_or(&[], |installed| &installed.entity_attributes)
}
/// Fixtures that tests of other crates use to install Parts.
#[cfg(any(test, feature = "testing"))]
pub mod tests {
    #[cfg(test)]
    use crate::*;

    #[test]
    pub(crate) fn entity_attribute_declarations_validate_and_refuse_duplicates() {
        let mut registrar = super::PartRegistrar::new("ruby");
        registrar
            .declare_entity_attributes(
                "ghoul",
                "minecraft:zombie",
                vec![("minecraft:max_health".to_string(), 55.0)],
            )
            .unwrap();
        // The same type twice is a duplicate, whatever declares it.
        assert_eq!(
            registrar.declare_entity_attributes("ghoul", "minecraft:zombie", vec![]),
            Err(EntityAttributeError::Duplicate)
        );
        // The path stays in the declaring mod's namespace.
        assert_eq!(
            registrar.declare_entity_attributes("foreign:ghoul", "minecraft:zombie", vec![]),
            Err(EntityAttributeError::InvalidId)
        );
        // A set validates itself: a duplicate attribute, a non-finite base.
        assert_eq!(
            registrar.declare_entity_attributes(
                "wraith",
                "minecraft:zombie",
                vec![
                    ("minecraft:max_health".to_string(), 1.0),
                    ("minecraft:max_health".to_string(), 2.0),
                ]
            ),
            Err(EntityAttributeError::DuplicateAttribute)
        );
        assert_eq!(
            registrar.declare_entity_attributes(
                "wraith",
                "minecraft:zombie",
                vec![("minecraft:max_health".to_string(), f64::NAN)]
            ),
            Err(EntityAttributeError::NotFinite)
        );
        // The refusals name the mod; the good declaration stands.
        assert_eq!(registrar.entity_attributes.len(), 1);
        assert_eq!(
            registrar.entity_attributes[0].attributes,
            [("minecraft:max_health".to_string(), 55.0)]
        );
        assert!(
            registrar
                .refused
                .iter()
                .all(|refusal| refusal.contains("mod ruby"))
        );
    }
}

/// Registrar operations for entity attributes.
pub trait EntityAttributesRegistrar {
    /// Declare the initial attribute set of the mod's entity type `path`
    /// (NeoForge's `EntityAttributeCreationEvent`), starting from `template`'s
    /// (`minecraft:zombie`, the one the type's content file copies, as
    /// `Zombie.createAttributes` builds on the template's): each attribute's
    /// id (`minecraft:max_health`) and the base value that differs from the
    /// template's. The values reach the type's own fields, its live attribute
    /// map and the attribute sync, so the reads of #1469 and #1493 answer the
    /// same defaults with no second copy.
    ///
    /// Resolved once, at load: an id outside the mod's namespace, a duplicate
    /// attribute, a non-finite base or a second declaration of the same type
    /// refuses the mods, naming the mod; an attribute the game does not know,
    /// or a template without attribute defaults, refuses the resource load,
    /// naming the type.
    fn declare_entity_attributes(
        &mut self,
        path: &str,
        template: &str,
        attributes: Vec<(String, f64)>,
    ) -> Result<(), crate::EntityAttributeError>;
}

impl EntityAttributesRegistrar for PartRegistrar {
    fn declare_entity_attributes(
        &mut self,
        path: &str,
        template: &str,
        attributes: Vec<(String, f64)>,
    ) -> Result<(), crate::EntityAttributeError> {
        let result = (|| {
            let (namespace, name) = path.split_once(':').unwrap_or((self.mod_id, path));
            if namespace.is_empty() || name.is_empty() || namespace != self.mod_id {
                return Err(crate::EntityAttributeError::InvalidId);
            }
            let (template_ns, template_name) =
                template.split_once(':').unwrap_or(("minecraft", template));
            if template_ns.is_empty() || template_name.is_empty() {
                return Err(crate::EntityAttributeError::InvalidId);
            }
            let id = format!("{namespace}:{name}");
            if self.entity_attributes.iter().any(|d| d.id == id) {
                return Err(crate::EntityAttributeError::Duplicate);
            }
            for (index, (attribute, base)) in attributes.iter().enumerate() {
                if !base.is_finite() {
                    return Err(crate::EntityAttributeError::NotFinite);
                }
                if attributes[..index]
                    .iter()
                    .any(|(other, _)| other == attribute)
                {
                    return Err(crate::EntityAttributeError::DuplicateAttribute);
                }
            }
            self.entity_attributes.push(EntityAttributeDeclaration {
                id,
                template: format!("{template_ns}:{template_name}"),
                attributes,
            });
            Ok(())
        })();
        if let Err(error) = &result {
            self.refused.push(format!(
                "mod {} entity attributes {path:?}: {error:?}",
                self.mod_id
            ));
        }
        result
    }
}
