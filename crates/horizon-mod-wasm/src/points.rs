//! Points use the hook transport and register in the native points list.
use crate::{CallClass, Refusal, WasmMod, WeakWasmMod, boundary::Frame};
use horizon_mod_package::manifest::Internal;
use horizon_mod_package::manifest::{
    Manifest,
    inject::{Access, Kind},
};
use mod_points::{
    __private::{Forward, PointHost, RawForward, SlotListener},
    PointId,
    endpoint_policy::{EndpointPolicy, InternalDeclaration, InternalsRefusal, SchemaCatalog},
    exposure::Plan,
};
use std::cell::RefCell;
use std::sync::Arc;

/// What the game supplies to resolve declared internals: its generated schema catalog and
/// each raw point's head call layout (the id its staged plan uses; not `PointId` order).
pub struct InternalsCatalog {
    pub schema: SchemaCatalog,
    pub head_calls: Vec<(PointId, usize)>,
}

/// The one mapping from a manifest's internals to catalog declarations.
fn declarations(internals: &[Internal]) -> Vec<InternalDeclaration> {
    internals
        .iter()
        .map(|internal| InternalDeclaration {
            path: internal.path.clone(),
            kind: internal.kind.clone(),
            hash: internal.hash.clone(),
        })
        .collect()
}

struct Endpoint {
    owner: &'static str,
    module: WeakWasmMod,
    listener: u32,
}

impl Forward for Endpoint {
    fn forward(&self, id: u32, host: PointHost<'_>, record: &mut dyn mod_api::wire::Record) {
        let mut frame = Frame::default();
        let mut side = None;
        match host {
            PointHost::Read(host) => frame.set_host(host),
            PointHost::Server(host) => frame.set_server_host_mut(host),
            PointHost::Client(host) => frame.set_client_host_mut(host),
            PointHost::Side(mod_api::HostSide::Server(host)) => {
                side = Some(true);
                frame.set_server_host_mut(host);
            }
            PointHost::Side(mod_api::HostSide::Client(host)) => {
                side = Some(false);
                frame.set_client_host_mut(host);
            }
            PointHost::None => {}
        }
        self.invoke(
            frame,
            id,
            |out| {
                if let Some(server) = side {
                    out.put(&server);
                }
            },
            record,
        );
    }
}

impl RawForward for Endpoint {
    fn plan(&self, head_call: usize) -> Option<Arc<Plan>> {
        self.module.plan(head_call)
    }
    fn forward(&self, id: u32, record: &mut dyn mod_api::wire::Record) {
        self.invoke(Frame::default(), id, |_| {}, record);
    }
}

impl Endpoint {
    fn invoke(
        &self,
        frame: Frame,
        id: u32,
        prefix: impl FnOnce(&mut mod_api::wire::Writer),
        record: &mut dyn mod_api::wire::Record,
    ) {
        let record = RefCell::new(record);
        self.module.call(
            CallClass::Hook,
            frame,
            id,
            None,
            |out| {
                out.put(&self.listener);
                prefix(out);
                record.borrow().encode(out);
            },
            |input| {
                input.take::<()>()?;
                let bytes = input.take_bytes(input.rest().len())?;
                record.borrow_mut().apply(bytes)
            },
        );
        if self.module.is_stopped() {
            mod_points::stop(self.owner);
        }
    }
}

/// Check subscriptions completely before registering any listener in the host's list.
pub(crate) fn listen(
    manifest: &Manifest,
    module: &WasmMod,
    catalog: Option<&InternalsCatalog>,
) -> Result<(), Refusal> {
    listen_checked(manifest, module, catalog).map_err(|error| match error {
        Listen::Internals(why) => Refusal::Internals(why),
        Listen::Other(why) => Refusal::Runtime(why),
    })
}
#[derive(Debug)]
pub(crate) enum Listen {
    Internals(InternalsRefusal),
    Other(String),
}
impl<T: Into<String>> From<T> for Listen {
    fn from(why: T) -> Self {
        Self::Other(why.into())
    }
}
/// Resolves the manifest's internals against the catalog for the raw points the module listens
/// to, and gives the module ownership of the resulting plans.
pub(crate) fn install_policy(
    manifest: &Manifest,
    module: &WasmMod,
    raw_places: &[PointId],
    catalog: Option<&InternalsCatalog>,
) -> Result<(), Listen> {
    let Some(catalog) = catalog else {
        return if manifest.internals.is_empty() {
            Ok(())
        } else {
            Err(Listen::Internals(InternalsRefusal::NoCatalog))
        };
    };
    let heads: Vec<usize> = raw_places
        .iter()
        .filter_map(|place| {
            catalog
                .head_calls
                .iter()
                .find(|(point, _)| point == place)
                .map(|(_, head)| *head)
        })
        .collect();
    let policy = EndpointPolicy::build(&catalog.schema, &declarations(&manifest.internals), &heads)
        .map_err(Listen::Internals)?;
    module.set_policy(policy);
    Ok(())
}
fn listen_checked(
    manifest: &Manifest,
    module: &WasmMod,
    catalog: Option<&InternalsCatalog>,
) -> Result<(), Listen> {
    if manifest.inject.is_empty() {
        return Ok(());
    }
    let listed: Vec<(u32, String)> = module
        .call(
            CallClass::Load,
            Frame::default(),
            0xBFFF_FFFF,
            None,
            |_| {},
            |input| input.take(),
        )
        .ok_or("point subscriptions did not answer")?;
    let mut declared = manifest
        .inject
        .iter()
        .map(|inject| (inject.id.as_str(), inject))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut slots = Vec::new();
    for (index, (place, id)) in listed.into_iter().enumerate() {
        let inject = declared
            .remove(id.as_str())
            .ok_or_else(|| format!("undeclared point listener {id}"))?;
        if place & 0x8000_0000 != 0 {
            return Err(format!("{id}: invalid point subscription id").into());
        }
        let raw = place & 0x4000_0000 != 0;
        let place = place & 0x3FFF_FFFF;
        // Raw places run only in a staged game, which installs its catalog; without one a raw
        // listener would never fire, so it is refused instead of silently registered.
        if raw && catalog.is_none() {
            return Err(Listen::Internals(InternalsRefusal::NoCatalog));
        }
        let catalog = if raw {
            mod_points::RAW_PLACES
        } else {
            mod_points::API_PLACES
        };
        let Some(&(point, target, at_return)) =
            catalog.iter().find(|(point, _, _)| *point as u32 == place)
        else {
            return Err(format!(
                "{id}: no generated {} snapshot/record for place {place}",
                if raw { "raw" } else { "API" }
            )
            .into());
        };
        if inject.access != if raw { Access::Internals } else { Access::Api }
            || !matches!(
                inject.kind,
                Kind::Head | Kind::ModifyArg | Kind::Replace | Kind::Return
            )
            || (inject.kind == Kind::Return) != at_return
            || horizon_mod_point_emit::plain_path(&inject.at)
                != horizon_mod_point_emit::plain_path(target)
        {
            return Err(format!(
                "{id}: listener does not match its declared API head/return target"
            )
            .into());
        }
        slots.push((point, index as u32, inject.priority, id, raw));
    }
    if let Some(id) = declared.keys().next() {
        return Err(format!("unfilled point listener {id}").into());
    }
    let raw_places: Vec<PointId> = slots
        .iter()
        .filter(|slot| slot.4)
        .map(|slot| slot.0)
        .collect();
    install_policy(manifest, module, &raw_places, catalog)?;
    let owner = Box::leak(manifest.id.clone().into_boxed_str());
    for (place, listener, priority, id, raw) in slots {
        let endpoint = Box::leak(Box::new(Endpoint {
            owner,
            module: module.downgrade(),
            listener,
        }));
        let id = Box::leak(id.into_boxed_str());
        let slot = if raw {
            SlotListener::snapshot(owner, place, endpoint, 0xC000_0000 + place as u32)
                .map_err(|error| error.to_string())?
        } else {
            SlotListener::forwarded(owner, place, endpoint, 0x8000_0000 + place as u32)
        };
        mod_points::register(&slot.ordered(priority, id)).map_err(|error| error.to_string())?;
    }
    Ok(())
}
