//! Native `[[inject]]` slot routing. The catalog owns validation; generation consumes checked entries.
use super::{inject::PointInjection, internals::Catalog, sites::Site};
use horizon_mod_package::{
    manifest::inject::{
        route, Access, CallPlace, Entry, Inject, InjectError, Kind, NoSlotReason, Place, Route,
        SlotKind, Target,
    },
    Manifest,
};

pub(super) fn points(manifest: &Manifest) -> impl Iterator<Item = PointInjection> + '_ {
    manifest
        .inject
        .iter()
        .filter(|entry| entry.access == Access::Internals && entry.call.is_none())
        .map(|entry| PointInjection {
            mod_id: manifest.id.clone(),
            id: entry.id.clone(),
            target: entry.at.clone(),
            method: entry.with.clone(),
            at_return: entry.kind == Kind::Return,
        })
}

pub(super) fn sites<'a>(
    sites: &'a [Site],
    manifest: &'a Manifest,
) -> impl Iterator<Item = (&'a Inject, &'a Site)> {
    manifest
        .inject
        .iter()
        .filter(|entry| entry.call.is_some())
        .map(|entry| {
            (
                entry,
                sites
                    .iter()
                    .find(|site| {
                        super::internals::plain_path(&site.target)
                            == super::internals::plain_path(&entry.at)
                            && Some(&site.selector) == entry.call.as_ref()
                            && Some(site.ordinal) == entry.nth
                    })
                    .expect("slot route checked"),
            )
        })
}

pub(super) fn check(catalog: &Catalog, manifest: &Manifest) -> Result<(), InjectError> {
    let plain = super::internals::plain_path;
    let places = manifest
        .inject
        .iter()
        .filter_map(|entry| {
            let item = catalog.internals.iter().find(|item| {
                !item.shape_only
                    && plain(&item.path) == plain(&entry.at)
                    && matches!(item.kind.as_str(), "fn" | "method" | "struct")
            })?;
            let point = catalog
                .points
                .iter()
                .find(|point| plain(&point.target) == plain(&item.path));
            Some(Place {
                at: entry.at.clone(),
                hash: item.hash.clone(),
                native_only: false,
                slots: point
                    .map(|point| {
                        let mut slots = Vec::new();
                        if entry.access == Access::Api || (point.raw && point.listener.is_some()) {
                            slots.push(SlotKind::Head);
                        }
                        if (entry.access == Access::Api && point.at_return)
                            || (entry.access == Access::Internals
                                && point.raw
                                && point.return_listener.is_some())
                        {
                            slots.push(SlotKind::Return);
                        }
                        slots
                    })
                    .unwrap_or_default(),
                calls: catalog
                    .sites
                    .iter()
                    .filter(|site| plain(&site.target) == plain(&item.path))
                    .map(|site| CallPlace {
                        call: site.selector.clone(),
                        nth: site.ordinal,
                        ctx: site.hash.clone(),
                        slots: vec![SlotKind::Site],
                    })
                    .collect(),
            })
        })
        .collect::<Vec<_>>();
    for inject in &manifest.inject {
        let route = route(
            Entry {
                inject,
                target: Target::Native,
                mod_id: &manifest.id,
                resolved_call: inject.call.as_deref(),
            },
            &places,
        )?;
        let pending = match (route, inject.kind, inject.access) {
            (Route::Source(_), _, _) => Some(NoSlotReason::SourcePending),
            (_, Kind::Replace, _) => Some(NoSlotReason::ReplacePending),
            (Route::Slot(SlotKind::Site), _, Access::Api) => Some(NoSlotReason::ApiSitesPending),
            _ => None,
        };
        if let Some(reason) = pending {
            return Err(InjectError::NoSlot {
                id: format!("{}:{}", manifest.id, inject.id),
                reason,
                declaration: format!("slots/{}.txt: {} ({route:?})", manifest.id, inject.at),
            });
        }
        if inject.kind == Kind::WrapIf
            && sites(&catalog.sites, manifest)
                .find(|(entry, _)| entry.id == inject.id)
                .expect("site checked")
                .1
                .output
                != "()"
        {
            return Err(InjectError::Invalid {
                id: format!("{}:{}", manifest.id, inject.id),
                field: "kind",
                fix: "wrap_if requires a unit-returning call",
            });
        }
    }
    Ok(())
}

pub(super) fn report(manifest: &Manifest) -> impl Iterator<Item = String> + '_ {
    manifest.inject.iter().map(|entry| {
        let form = if entry.call.is_some() {
            "site"
        } else if entry.kind == Kind::Return {
            "return"
        } else {
            "head"
        };
        format!(
            "{}:{}: {} -> slot ({form}, {})",
            manifest.id,
            entry.id,
            entry.at,
            if entry.access == Access::Api {
                "api"
            } else {
                "internals"
            }
        )
    })
}

#[cfg(test)]
mod tests;
