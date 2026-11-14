//! Resolve bundle dependencies without touching the staged workspace.
use crate::manifest::Side;
use semver::{Version, VersionReq};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::Path;

/// Parse an installed mod version, padding omitted minor and patch components.
/// This is also the comparison used for saved-world mod versions.
pub fn parse_version(version: &str) -> Result<Version, semver::Error> {
    let padded = match version.matches('.').count() {
        0 => format!("{version}.0.0"),
        1 => format!("{version}.0"),
        _ => version.to_owned(),
    };
    Version::parse(&padded)
}

#[derive(Debug)]
pub enum Error {
    Manifest {
        owner: String,
        target: String,
        reason: String,
    },
    Duplicate(String),
    Missing {
        owner: String,
        target: String,
    },
    NoApi {
        owner: String,
        target: String,
    },
    CargoConflict {
        owner: String,
        target: String,
    },
    Version {
        owner: String,
        target: String,
        version: String,
        requirement: String,
    },
    Cycle {
        mods: Vec<String>,
    },
    Incompatible {
        owner: String,
        target: String,
        version: String,
        /// The owner's reason and link, ` (reason, url)`, or empty.
        note: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest {
                owner,
                target,
                reason,
            } => write!(f, "{owner}: dependency {target}: {reason}"),
            Self::Duplicate(id) => write!(f, "Duplicate mod id {id:?}"),
            Self::Missing { owner, target } => write!(f, "{owner} requires missing mod {target}"),
            Self::NoApi { owner, target } => {
                write!(f, "{owner} depends on {target}, which has no [api] package")
            }
            Self::CargoConflict { owner, target } => {
                write!(
                    f,
                    "{owner}: dependency {target} conflicts with its Cargo manifest"
                )
            }
            Self::Version {
                owner,
                target,
                version,
                requirement,
            } => write!(
                f,
                "{owner} requires {target} {requirement}, but installed version is {version}"
            ),
            Self::Cycle { mods } => write!(f, "Mod dependency cycle: {}", mods.join(" -> ")),
            Self::Incompatible {
                owner,
                target,
                version,
                note,
            } => write!(f, "{owner} is incompatible with {target} {version}{note}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Dependency {
    pub id: String,
    pub requirement: VersionReq,
    pub optional: bool,
    pub side: Side,
    pub reason: Option<String>,
    pub url: Option<String>,
    pub order: mod_vocab::DependencyOrder,
}

pub fn from_manifest(
    values: &BTreeMap<String, crate::manifest::Dependency>,
    running: Side,
) -> Vec<Dependency> {
    values
        .iter()
        .filter_map(|(id, value)| {
            let details = value.details();
            if running != Side::Both && details.side != Side::Both && details.side != running {
                return None;
            }
            Some(Dependency {
                id: id.clone(),
                requirement: details.version,
                optional: details.optional,
                side: details.side,
                reason: details.reason,
                url: details.url,
                order: details.order,
            })
        })
        .collect()
}

#[cfg(test)]
fn parse(owner: &str, value: Option<&toml::Value>) -> Result<Vec<Dependency>, Error> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let manifest = format!(
        "id='test'\nname='Test'\nversion='1'\napi='0.2'\ncapabilities=['registries']\n{}\n[native]\ngame='26.3'\nhorizon='{}'\nmixins='{}'\n",
        toml::to_string(&toml::Table::from_iter([(
            "dependencies".into(),
            value.clone()
        )]))
        .unwrap(),
        "a".repeat(40),
        "b".repeat(40)
    );
    let manifest =
        crate::manifest::Manifest::parse(&manifest).map_err(|reason| Error::Manifest {
            owner: owner.into(),
            target: "[dependencies]".into(),
            reason,
        })?;
    Ok(from_manifest(
        &manifest.dependencies,
        crate::manifest::Side::Both,
    ))
}

#[derive(Clone)]
pub struct Mod<'a> {
    pub id: &'a str,
    pub version: &'a str,
    pub has_api: bool,
    pub dependencies: &'a [Dependency],
}

#[derive(Debug)]
pub struct Resolution {
    /// Original bundle indices, in dispatch order.
    pub order: Vec<usize>,
    /// What the load rules let through: a discouraged pair, an override applied.
    pub warnings: Vec<String>,
}

pub fn resolve(mods: &[Mod<'_>]) -> Result<Resolution, Error> {
    resolve_with_api(mods, true, &[], &Overrides::default())
}

/// Resolve content layers without requiring native API packages.
pub fn resolve_content(mods: &[Mod<'_>]) -> Result<Resolution, Error> {
    resolve_with_api(mods, false, &[], &Overrides::default())
}

/// [`resolve`], with `rules[i]` the load rules of `mods[i]` and an instance's overrides, as the
/// game starts its code mods.
pub fn resolve_with_rules(
    mods: &[Mod<'_>],
    rules: &[Vec<Rule>],
    overrides: &Overrides,
) -> Result<Resolution, Error> {
    resolve_with_api(mods, true, rules, overrides)
}

/// What a load rule asks of another mod.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleKind {
    /// `[incompatible]`: refuse to start beside it.
    Incompatible,
    /// `[discouraged]`: start beside it with a warning.
    Discouraged,
    /// `load_before`: run before it when it is loaded.
    LoadBefore,
}

/// One of a mod's load rules about another mod, on the running side.
#[derive(Clone, Debug)]
pub struct Rule {
    pub kind: RuleKind,
    pub id: String,
    /// The other mod's versions the rule matches.
    pub requirement: VersionReq,
    /// The mod's reason and link, ` (reason, url)`, or empty.
    pub note: String,
}

/// `manifest`'s load rules that hold on `running`, the client or the dedicated server.
pub fn rules(manifest: &crate::Manifest, running: Side) -> Vec<Rule> {
    fn declared<'a>(
        kind: RuleKind,
        table: &'a BTreeMap<String, crate::manifest::Rule>,
        running: Side,
    ) -> impl Iterator<Item = Rule> + 'a {
        table.iter().filter_map(move |(id, rule)| {
            let rule = rule.details();
            let note = match (rule.reason, rule.url) {
                (Some(reason), Some(url)) => format!(" ({reason}, {url})"),
                (Some(text), None) | (None, Some(text)) => format!(" ({text})"),
                (None, None) => String::new(),
            };
            (rule.side == Side::Both || rule.side == running).then(|| Rule {
                kind,
                id: id.clone(),
                requirement: rule.version,
                note,
            })
        })
    }
    declared(RuleKind::Incompatible, &manifest.incompatible, running)
        .chain(declared(
            RuleKind::Discouraged,
            &manifest.discouraged,
            running,
        ))
        .chain(manifest.load_before.iter().map(|id| Rule {
            kind: RuleKind::LoadBefore,
            id: id.clone(),
            requirement: VersionReq::STAR,
            note: String::new(),
        }))
        .collect()
}

/// What an instance's overrides file does to one mod's rule about another (NeoForge's
/// `FMLConfig.DependencyOverride`).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Override {
    /// Ignore the rule.
    Drop,
    /// Load beside an incompatible mod, with a warning.
    Discourage,
}

/// The overrides file's name, in the instance's `horizon-mods` folder.
pub const OVERRIDES: &str = "dependency-overrides.toml";

/// An instance's overrides of the mods' load rules: `[owner]` tables of `target = "drop"` or
/// `"discourage"`.
#[derive(Debug, Default, Deserialize)]
#[serde(transparent)]
pub struct Overrides(BTreeMap<String, BTreeMap<String, Override>>);

impl Overrides {
    /// The overrides file in `mods_dir`; none when there is no file.
    pub fn read(mods_dir: &Path) -> Result<Self, String> {
        let path = mods_dir.join(OVERRIDES);
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    fn get(&self, owner: &str, target: &str) -> Option<Override> {
        self.0.get(owner)?.get(target).copied()
    }
}

fn resolve_with_api(
    mods: &[Mod<'_>],
    require_api: bool,
    rules: &[Vec<Rule>],
    overrides: &Overrides,
) -> Result<Resolution, Error> {
    let mut ids = HashMap::new();
    for (index, item) in mods.iter().enumerate() {
        if ids.insert(item.id, index).is_some() {
            return Err(Error::Duplicate(item.id.into()));
        }
    }
    let mut edges = vec![Vec::new(); mods.len()];
    for (index, item) in mods.iter().enumerate() {
        for dep in item.dependencies {
            let Some(&target) = ids.get(dep.id.as_str()) else {
                if dep.optional {
                    continue;
                }
                return Err(Error::Missing {
                    owner: item.id.into(),
                    target: dep.id.clone(),
                });
            };
            let other = &mods[target];
            if require_api && (!item.has_api || !other.has_api) {
                return Err(Error::NoApi {
                    owner: item.id.into(),
                    target: dep.id.clone(),
                });
            }
            // A `[mod] version` may leave out minor and patch (`"1"`, `"1.2"`), as Cargo's
            // requirements do: they count as zero.
            let version = parse_version(other.version).map_err(|e| Error::Manifest {
                owner: item.id.into(),
                target: dep.id.clone(),
                reason: format!("invalid installed version {:?}: {e}", other.version),
            })?;
            if !dep.requirement.matches(&version) {
                return Err(Error::Version {
                    owner: item.id.into(),
                    target: dep.id.clone(),
                    version: other.version.into(),
                    requirement: dep.requirement.to_string(),
                });
            }
            edges[index].push(target);
        }
    }
    let mut warnings = Vec::new();
    for (index, item) in mods.iter().enumerate() {
        for rule in rules.get(index).into_iter().flatten() {
            // A rule about a mod that isn't loaded holds nothing.
            let Some(&target) = ids.get(rule.id.as_str()) else {
                continue;
            };
            let mut kind = rule.kind;
            match overrides.get(item.id, &rule.id) {
                Some(Override::Drop) => {
                    warnings.push(format!(
                        "dependency override: {} -> {} dropped",
                        item.id, rule.id
                    ));
                    continue;
                }
                Some(Override::Discourage) if kind == RuleKind::Incompatible => {
                    warnings.push(format!(
                        "dependency override: {} -> {} only discouraged",
                        item.id, rule.id
                    ));
                    kind = RuleKind::Discouraged;
                }
                _ => {}
            }
            if kind == RuleKind::LoadBefore {
                edges[target].push(index);
                continue;
            }
            let other = &mods[target];
            let version = parse_version(other.version).map_err(|e| Error::Manifest {
                owner: item.id.into(),
                target: rule.id.clone(),
                reason: format!("invalid installed version {:?}: {e}", other.version),
            })?;
            if !rule.requirement.matches(&version) {
                continue;
            }
            if kind == RuleKind::Incompatible {
                return Err(Error::Incompatible {
                    owner: item.id.into(),
                    target: rule.id.clone(),
                    version: other.version.into(),
                    note: rule.note.clone(),
                });
            }
            warnings.push(format!(
                "{} works badly with {} {}{}",
                item.id, rule.id, other.version, rule.note
            ));
        }
    }
    let mut done = vec![false; mods.len()];
    let mut order = Vec::with_capacity(mods.len());
    while order.len() < mods.len() {
        let next = (0..mods.len()).find(|&i| !done[i] && edges[i].iter().all(|&d| done[d]));
        let Some(next) = next else {
            // Follow remaining edges until a node repeats, reporting the actual cycle only.
            let mut seen = BTreeMap::new();
            let mut path: Vec<usize> = Vec::new();
            let mut current = done.iter().position(|done| !done).expect("unfinished node");
            loop {
                if let Some(&start) = seen.get(&current) {
                    let mut cycle: Vec<String> = path[start..]
                        .iter()
                        .map(|&i| mods[i].id.to_owned())
                        .collect();
                    cycle.push(mods[current].id.to_owned());
                    return Err(Error::Cycle { mods: cycle });
                }
                seen.insert(current, path.len());
                path.push(current);
                current = *edges[current]
                    .iter()
                    .find(|&&i| !done[i])
                    .expect("unfinished dependency");
            }
        };
        done[next] = true;
        order.push(next);
    }
    Ok(Resolution { order, warnings })
}

/// A target's staged Cargo package, or none for an absent optional mod.
pub struct Package<'a> {
    pub dependency: &'a Dependency,
    pub package: Option<&'a str>,
}

/// Add bundle edges to the dependent package. Returns features to enable on the game edge.
pub fn rewrite(
    owner: &str,
    manifest: &mut toml::Table,
    packages: &[Package<'_>],
) -> Result<Vec<String>, Error> {
    let mut updated = manifest.clone();
    let mut enabled = Vec::new();
    for target in packages {
        let id = &target.dependency.id;
        let normalized = id.replace('-', "_");
        let conflict = || Error::CargoConflict {
            owner: owner.into(),
            target: id.clone(),
        };
        let dependencies = updated
            .entry("dependencies")
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .ok_or_else(conflict)?;
        if dependencies
            .keys()
            .any(|key| key.replace('-', "_") == normalized)
        {
            return Err(conflict());
        }
        if let Some(package) = target.package {
            let mut entry = toml::Table::new();
            entry.insert("path".into(), format!("../{id}").into());
            entry.insert("package".into(), package.into());
            if target.dependency.optional {
                entry.insert("optional".into(), true.into());
            }
            dependencies.insert(id.clone(), toml::Value::Table(entry));
        } else if !target.dependency.optional {
            return Err(Error::Missing {
                owner: owner.into(),
                target: id.clone(),
            });
        }
        if target.dependency.optional {
            let features = updated
                .entry("features")
                .or_insert_with(|| toml::Value::Table(toml::Table::new()))
                .as_table_mut()
                .ok_or_else(conflict)?;
            if features.contains_key(id)
                || features.values().any(|value| {
                    value.as_array().is_some_and(|entries| {
                        entries.iter().any(|entry| {
                            entry.as_str().is_some_and(|s| {
                                s == id
                                    || s == format!("dep:{id}")
                                    || s.starts_with(&format!("{id}/"))
                                    || s.starts_with(&format!("{id}?/"))
                            })
                        })
                    })
                })
            {
                return Err(conflict());
            }
            let entries = if target.package.is_some() {
                enabled.push(id.clone());
                vec![toml::Value::String(format!("dep:{id}"))]
            } else {
                Vec::new()
            };
            features.insert(id.clone(), toml::Value::Array(entries));
        }
    }
    *manifest = updated;
    Ok(enabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn deps(text: &str) -> Vec<Dependency> {
        let value: toml::Table = text.parse().unwrap();
        parse("user", value.get("dependencies")).unwrap()
    }
    fn item<'a>(id: &'a str, version: &'a str, dependencies: &'a [Dependency]) -> Mod<'a> {
        Mod {
            id,
            version,
            has_api: true,
            dependencies,
        }
    }
    #[test]
    fn content_dependencies_validate_and_order_without_native_api_packages() {
        let manifest = |id: &str, version: &str, dependencies: &str| {
            crate::Manifest::parse(&format!(
                "id='{id}'\nname='{id}'\nversion='{version}'\napi='0.2'\ncapabilities=['registries']\n{dependencies}"
            )).unwrap()
        };
        let addon = manifest(
            "addon",
            "1",
            "[dependencies]\nbase='^1'\nextra={version='^2', optional=true}",
        );
        let base = manifest("base", "1.2", "");
        let dependencies = from_manifest(&addon.dependencies, crate::manifest::Side::Both);
        let mods = [
            Mod {
                id: &addon.id,
                version: &addon.version,
                has_api: false,
                dependencies: &dependencies,
            },
            Mod {
                id: &base.id,
                version: &base.version,
                has_api: false,
                dependencies: &[],
            },
        ];
        assert_eq!(resolve_content(&mods).unwrap().order, [1, 0]);
        assert!(matches!(resolve(&mods), Err(Error::NoApi { .. })));
        assert!(matches!(
            resolve_content(&mods[..1]),
            Err(Error::Missing { .. })
        ));
        let incompatible = Mod {
            id: "base",
            version: "2",
            has_api: false,
            dependencies: &[],
        };
        assert!(matches!(
            resolve_content(&[mods[0].clone(), incompatible]),
            Err(Error::Version { .. })
        ));
        let back = from_manifest(
            &manifest("base", "1", "[dependencies]\naddon='^1'").dependencies,
            crate::manifest::Side::Both,
        );
        assert!(matches!(
            resolve_content(&[
                mods[0].clone(),
                Mod {
                    id: "base",
                    version: "1",
                    has_api: false,
                    dependencies: &back
                }
            ]),
            Err(Error::Cycle { .. })
        ));
    }

    #[test]
    fn load_rules_refuse_warn_order_and_yield_to_overrides() {
        let header =
            "id='user'\nname='User'\nversion='1'\napi='0.2'\ncapabilities=['registries']\n";
        let user = crate::Manifest::parse(&format!(
            "{header}load_before=['late', 'absent']\n[incompatible]\n\
             broken={{version='<2', reason='Corrupts saves', url='https://example.com/why'}}\n\
             client_only={{side='client'}}\n[discouraged]\nslow='*'\n"
        ))
        .unwrap();
        let server = rules(&user, Side::Server);
        let client = rules(&user, Side::Client);
        let none = Overrides::default();
        let with = |others: &[(&'static str, &'static str)], rules: &[Rule], overrides| {
            let mut mods = vec![item("user", "1", &[])];
            mods.extend(others.iter().map(|&(id, version)| item(id, version, &[])));
            let rules = [rules.to_vec()];
            resolve_with_rules(&mods, &rules, overrides)
        };
        let refused = with(&[("broken", "1.5")], &server, &none).unwrap_err();
        assert_eq!(
            refused.to_string(),
            "user is incompatible with broken 1.5 (Corrupts saves, https://example.com/why)"
        );
        assert!(with(&[("broken", "2.0")], &server, &none).is_ok());
        // A client-only rule holds nothing on a dedicated server.
        assert!(with(&[("client_only", "1")], &server, &none).is_ok());
        assert!(with(&[("client_only", "1")], &client, &none).is_err());
        let slow = with(&[("slow", "3")], &server, &none).unwrap();
        assert_eq!(slow.warnings, ["user works badly with slow 3"]);
        // Before a present mod it doesn't depend on; an absent one is ignored.
        let mods = [item("late", "1", &[]), item("user", "1", &[])];
        let rules = [Vec::new(), server.clone()];
        assert_eq!(
            resolve_with_rules(&mods, &rules, &none).unwrap().order,
            [1, 0]
        );
        let discourage: Overrides = toml::from_str("[user]\nbroken = 'discourage'\n").unwrap();
        let warned = with(&[("broken", "1.5")], &server, &discourage).unwrap();
        assert_eq!(
            warned.warnings,
            [
                "dependency override: user -> broken only discouraged",
                "user works badly with broken 1.5 (Corrupts saves, https://example.com/why)"
            ]
        );
        let drop: Overrides = toml::from_str("[user]\nbroken = 'drop'\n").unwrap();
        let dropped = with(&[("broken", "1.5")], &server, &drop).unwrap();
        assert_eq!(
            dropped.warnings,
            ["dependency override: user -> broken dropped"]
        );
        for refused in [
            "[incompatible]\nx={url='http://example.com'}",
            "load_before=['Bad']",
        ] {
            assert!(crate::Manifest::parse(&format!("{header}{refused}")).is_err());
        }
    }

    #[test]
    fn order_and_ties_include_present_optional() {
        let user = deps("[dependencies]\nbase = '^1'\nextra = { version = '^2', optional = true }");
        let mods = [
            item("user", "1.0.0", &user),
            item("base", "1.2", &[]),
            item("extra", "2", &[]),
            item("other", "1.0.0", &[]),
        ];
        let result = resolve(&mods).unwrap();
        assert_eq!(result.order, [1, 2, 0, 3]);
        assert_eq!(
            resolve(&[item("user", "1.0.0", &user), item("base", "1.2.0", &[])])
                .unwrap()
                .order,
            [1, 0]
        );
    }
    #[test]
    fn failures_name_mods() {
        let required = deps("[dependencies]\nbase = '^1'");
        let optional = deps("[dependencies]\nbase = { version = '^1', optional = true }");
        for dependencies in [&required, &optional] {
            let err = resolve(&[
                item("user", "1.0.0", dependencies),
                item("base", "2.0.0", &[]),
            ])
            .err()
            .unwrap()
            .to_string();
            assert!(err.contains("user") && err.contains("base"));
        }
        let err = resolve(&[item("user", "1.0.0", &required)])
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("user") && err.contains("base"));
        let back = deps("[dependencies]\nuser = '^1'");
        let err = resolve(&[
            item("user", "1.0.0", &required),
            item("base", "1.0.0", &back),
        ])
        .err()
        .unwrap()
        .to_string();
        assert!(err.contains("user") && err.contains("base"));
    }
    #[test]
    fn rewrites_installed_and_absent_optional_dependencies() {
        let deps = deps("[dependencies]\nbase = '^1'\nextra = { version = '^2', optional = true }");
        let mut manifest: toml::Table = "[dependencies]\nhorizon-mod-api = '1'".parse().unwrap();
        let packages = [
            Package {
                dependency: &deps[0],
                package: Some("base-package"),
            },
            Package {
                dependency: &deps[1],
                package: Some("extra-package"),
            },
        ];
        assert_eq!(
            rewrite("user", &mut manifest, &packages).unwrap(),
            ["extra"]
        );
        assert_eq!(
            manifest["dependencies"]["base"]["path"].as_str(),
            Some("../base")
        );
        assert_eq!(
            manifest["dependencies"]["base"]["package"].as_str(),
            Some("base-package")
        );
        assert_eq!(
            manifest["dependencies"]["extra"]["optional"].as_bool(),
            Some(true)
        );
        assert_eq!(manifest["features"]["extra"][0].as_str(), Some("dep:extra"));
        let mut absent = toml::Table::new();
        assert!(
            rewrite(
                "user",
                &mut absent,
                &[Package {
                    dependency: &deps[1],
                    package: None
                }]
            )
            .unwrap()
            .is_empty()
        );
        assert!(absent["features"]["extra"].as_array().unwrap().is_empty());
        assert!(absent["dependencies"].get("extra").is_none());
    }

    #[test]
    fn rejects_conflicting_features_and_dependency_names_atomically() {
        let deps = deps("[dependencies]\nextra = { version = '^2', optional = true }");
        for text in [
            "[features]\ndefault = ['extra']",
            "[dependencies]\nextra = '1'",
        ] {
            let mut manifest: toml::Table = text.parse().unwrap();
            let original = manifest.clone();
            assert!(
                rewrite(
                    "user",
                    &mut manifest,
                    &[Package {
                        dependency: &deps[0],
                        package: Some("extra-package")
                    }]
                )
                .is_err()
            );
            assert_eq!(manifest, original);
        }
    }

    #[test]
    fn malformed_and_no_api() {
        let value: toml::Table = "[dependencies]\nbase = { version = '^1', optional = 'yes' }"
            .parse()
            .unwrap();
        assert!(parse("user", value.get("dependencies")).is_err());
        let required = deps("[dependencies]\nbase = '^1'");
        let mut base = item("base", "1.0.0", &[]);
        base.has_api = false;
        assert!(matches!(
            resolve(&[item("user", "1.0.0", &required), base]),
            Err(Error::NoApi { .. })
        ));
    }
}
