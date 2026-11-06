//! Wasm mods listening at the game's points (#589): a `[wasm]` package's `[[native.injection]]`
//! entries name the places its `mod.wasm` fills. At load each is checked against this game's
//! points and against the mod's own listeners, then the mod registers in the points crate's one
//! list of listeners, exactly as a native mod does: its places fire it through
//! `mod_points::wire::forwarded`, as hook calls within its group's limits.
use crate::Refusal;
use horizon_mods::manifest::{InjectionKind, Manifest};
use mod_api::__private::wasm_host::{CallHost, Exports};
use mod_points::__private::wire::{Forward, HostKind, PointHost, Record, Value, forwarded};
use mod_points::PointId;
use mod_points::places::{IDS, NAMES, SHAPES, TARGETS};
use std::collections::BTreeSet;

type Subscribed = wasmtime::component::TypedFunc<(), (Vec<(String, String)>,)>;
type Fire = wasmtime::component::TypedFunc<(u32, HostKind, Vec<Value>), (Vec<Value>,)>;

/// One wasm mod's listeners: its `listeners` export, and the places it fills in the order the
/// mod named them (a place's index there is the one `fire` takes).
struct Listeners {
    exports: Exports,
    fire: Fire,
    places: &'static [Option<PointId>],
}

impl Forward for Listeners {
    fn subscribed(&self) -> &'static [Option<PointId>] {
        self.places
    }

    fn forward(&self, place: PointId, host: PointHost<'_>, record: &mut dyn Record) {
        let Some(index) = self.places.iter().position(|p| *p == Some(place)) else {
            return;
        };
        let (host, wire) = match host {
            PointHost::Read(h) => (CallHost::Read(h), HostKind::Read),
            PointHost::Server(h) | PointHost::Side(mod_api::HostSide::Server(h)) => {
                (CallHost::Server(h), HostKind::Server)
            }
            PointHost::Client(h) | PointHost::Side(mod_api::HostSide::Client(h)) => {
                (CallHost::Client(h), HostKind::Client)
            }
            PointHost::None => (CallHost::None, HostKind::None),
        };
        let fields = record.lower();
        let answer = self.exports.call(host, |store| {
            Ok(self.fire.call(store, (index as u32, wire, fields))?.0)
        });
        let written = match answer {
            Some(fields) => record.write_back(fields),
            None if self.exports.stopped() => {
                mod_points::stop(self.exports.id());
                return;
            }
            // Refused (busy, or a call back into its own group): the place runs as if it
            // weren't listened at, as a hook's would.
            None => return,
        };
        if let Err(why) = written {
            log::error!(
                "wasm mod {} stopped listening at points: at {}, {why}",
                self.exports.id(),
                NAMES[place as usize]
            );
            mod_points::stop(self.exports.id());
        }
    }
}

/// The place a manifest's injection names: the point at its `target`, at the head or return its
/// `kind` says, whose `Points` method its `trait_method` must be (`<point>` at the head,
/// `<point>_return` at the return).
fn declared_place(target: &str, method: &str, kind: InjectionKind) -> Result<usize, Refusal> {
    let missing = || Refusal::NoSuchPoint {
        point: format!("{target} ({method})"),
    };
    // A point's head comes first among its places, its return right after.
    let head = TARGETS
        .iter()
        .position(|t| *t == target)
        .ok_or_else(missing)?;
    let place = match kind {
        InjectionKind::Return => Some(head + 1)
            .filter(|&back| TARGETS.get(back) == Some(&target))
            .ok_or_else(missing)?,
        _ => head,
    };
    if NAMES[place] != method {
        return Err(missing());
    }
    Ok(place)
}

/// A wasm mod's checked listeners, registered once every native mod's are, in the installed order.
/// It owns them until then: one dropped unregistered (its mod refused after the check) lets its
/// group go.
pub struct Listening {
    exports: Exports,
    fire: Fire,
    places: Vec<Option<PointId>>,
}

impl Listening {
    /// Registers the listeners in the points crate's one list, which keeps them for good.
    pub fn register(self) -> Result<(), mod_points::RegisterError> {
        let id = self.exports.id();
        let listeners = Listeners {
            exports: self.exports,
            fire: self.fire,
            places: Box::leak(self.places.into_boxed_slice()),
        };
        mod_points::register(id, Box::leak(forwarded(Box::new(listeners))))
    }
}

/// Checks a loaded wasm mod's listeners against its manifest and this game's points: the listeners
/// to register, none for a mod whose manifest lists no injections.
pub(crate) fn listen(manifest: &Manifest, exports: &Exports) -> Result<Option<Listening>, Refusal> {
    let Some(wasm) = &manifest.wasm else {
        return Ok(None);
    };
    if wasm.points().is_empty() {
        return Ok(None);
    }
    let mut declared = BTreeSet::new();
    for injection in wasm.points() {
        // The manifest admits only `trait_method` injections with a `target` beside `[wasm]`.
        let (Some(target), Some(method)) = (&injection.target, &injection.trait_method) else {
            continue;
        };
        declared.insert(declared_place(target, method, injection.kind)?);
    }
    let interface = format!(
        "{}-listeners",
        horizon_mods::manifest::export_prefix(&manifest.id)
    );
    let subscribed: Subscribed = exports
        .func(&interface, "subscribed")
        .map_err(Refusal::Load)?;
    let fire: Fire = exports.func(&interface, "fire").map_err(Refusal::Load)?;
    let listed = exports
        .load(|store| Ok(subscribed.call(store, ())?.0))
        .ok_or_else(|| {
            Refusal::Points("its listeners didn't answer which places they fill".into())
        })?;
    let mut places = Vec::with_capacity(listed.len());
    for (name, shape) in &listed {
        let Some(place) = NAMES.iter().position(|place| place == name) else {
            return Err(Refusal::NoSuchPoint {
                point: name.clone(),
            });
        };
        if !declared.remove(&place) {
            return Err(Refusal::UndeclaredPoint {
                point: name.clone(),
            });
        }
        if SHAPES[place] != shape {
            return Err(Refusal::ChangedPoint {
                point: name.clone(),
                built: shape.clone(),
                game: SHAPES[place].to_owned(),
            });
        }
        places.push(Some(IDS[place]));
    }
    if let Some(&place) = declared.first() {
        return Err(Refusal::UnfilledPoint {
            point: NAMES[place].to_owned(),
        });
    }
    Ok(Some(Listening {
        exports: exports.clone(),
        fire,
        places,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: &str = "horizon_client::game::Game<'_>::tick";

    #[test]
    fn an_injection_names_the_place_its_kind_and_method_say() {
        let head = declared_place(TICK, "tick", InjectionKind::Head).unwrap();
        let back = declared_place(TICK, "tick_return", InjectionKind::Return).unwrap();
        assert_eq!((NAMES[head], NAMES[back]), ("tick", "tick_return"));
        // The method must be the one the kind places at the target.
        for (method, kind) in [
            ("tick_return", InjectionKind::Head),
            ("tick", InjectionKind::Return),
            ("tick_world", InjectionKind::Head),
        ] {
            assert!(
                matches!(
                    declared_place(TICK, method, kind),
                    Err(Refusal::NoSuchPoint { .. })
                ),
                "{method} at {kind:?}"
            );
        }
        assert!(
            declared_place(
                "horizon_client::game::Game<'_>::gone",
                "gone",
                InjectionKind::Head
            )
            .is_err()
        );
    }
}
