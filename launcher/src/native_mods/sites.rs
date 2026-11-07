//! Raw call-site routing and adapters; every kind uses the same typed wrap constructor.
use super::internals::Error;
use horizon_mod_package::{
    manifest::{Injection, InjectionKind},
    Manifest,
};
use std::collections::BTreeSet;

#[derive(Clone, Debug, serde::Deserialize)]
pub(super) struct Site {
    pub name: String,
    pub target: String,
    pub selector: String,
    pub ordinal: usize,
    pub hash: String,
    pub listener: String,
    pub record: String,
    pub output: String,
}

pub(super) fn of(manifest: &Manifest) -> impl Iterator<Item = &Injection> {
    manifest
        .native
        .iter()
        .flat_map(|n| &n.injection)
        .filter(|i| i.raw.is_some() && i.selector.is_some())
}

pub(super) fn find<'a>(sites: &'a [Site], injection: &Injection) -> Option<&'a Site> {
    let target = super::internals::plain_path(injection.target.as_deref().expect("site target"));
    let selector = injection
        .selector
        .as_deref()
        .expect("site selector")
        .replace(' ', "");
    sites.iter().find(|site| {
        site.target == target
            && site.selector == selector
            && site.ordinal == injection.ordinal.unwrap_or(0)
    })
}

pub(super) fn check<'a>(
    sites: &'a [Site],
    manifest: &Manifest,
    injection: &Injection,
) -> Result<&'a Site, Error> {
    let site = find(sites, injection).ok_or_else(|| Error::NotSite {
        mod_id: manifest.id.clone(),
        line: format!(
            "{} | {} | {} |",
            injection.target.as_deref().expect("target"),
            injection.selector.as_deref().expect("selector"),
            injection.ordinal.unwrap_or(0)
        ),
    })?;
    if injection
        .fingerprint
        .as_ref()
        .is_some_and(|hash| hash != &site.hash)
    {
        return Err(Error::Changed {
            mod_id: manifest.id.clone(),
            path: format!("{} | {} | {}", site.target, site.selector, site.ordinal),
            signature: site.hash.clone(),
        });
    }
    if injection.kind == InjectionKind::WrapWithCondition && site.output != "()" {
        return Err(Error::SiteKind {
            mod_id: manifest.id.clone(),
            injection: injection.id.clone(),
        });
    }
    Ok(site)
}

impl Site {
    pub(super) fn constructor(&self) -> String {
        self.name.to_ascii_lowercase()
    }

    pub(super) fn facade(
        &self,
        exports: &mut BTreeSet<String>,
        constructors: &mut BTreeSet<String>,
        crates: &mut BTreeSet<String>,
    ) {
        crates.insert(
            self.listener
                .split("::")
                .next()
                .expect("listener crate")
                .into(),
        );
        let prefix = self
            .listener
            .strip_suffix("Listener")
            .expect("listener suffix");
        for suffix in ["Listener", "Receiver", "Output"] {
            exports.insert(format!("    pub use {prefix}{suffix};\n"));
        }
        exports.insert(format!("    pub use {};\n", self.record));
        let listener = self.listener.rsplit("::").next().expect("listener name");
        constructors.insert(format!("    /// Registers this site's exact typed wrap listener.\n    pub const fn {}(id: &'static str, priority: i32, injection: &'static str, f: {listener}) -> mod_points::__private::SlotListener {{\n        // SAFETY: the function's exact type is checked before erasure.\n        unsafe {{ mod_points::__private::SlotListener::site(id, mod_points::SiteId::{}, priority, injection, f as *const ()) }}\n    }}\n", self.constructor(), self.name));
    }

    pub(super) fn registration(
        &self,
        index: usize,
        manifest: &Manifest,
        injection: &Injection,
    ) -> String {
        let f = format!("mod_{index}::{}", injection.raw.as_ref().expect("raw site"));
        use horizon_mod_package::manifest::inject::Kind;
        let kind = match injection.kind {
            InjectionKind::BeforeCall => Kind::BeforeCall,
            InjectionKind::ModifyArg => Kind::ModifyCallArg,
            InjectionKind::AfterCall => Kind::AfterCall,
            InjectionKind::ModifyExpressionValue => Kind::ModifyValue,
            InjectionKind::WrapOperation => Kind::WrapCall,
            InjectionKind::WrapWithCondition => Kind::WrapIf,
            _ => unreachable!("manifest checked a site kind"),
        };
        let adapter = adapter(kind, &f);
        format!(
            "        mod_{index}_internals::raw::{}({:?}, {}, {:?}, {adapter}),\n",
            self.constructor(),
            manifest.id,
            injection.priority.unwrap_or(0),
            injection.id
        )
    }
}

pub(super) fn adapter(kind: horizon_mod_package::manifest::inject::Kind, f: &str) -> String {
    use horizon_mod_package::manifest::inject::Kind;
    match kind {
        Kind::BeforeCall | Kind::ModifyCallArg => format!("|receiver, args, original| {{ {f}(receiver, args); original(receiver, args) }}"),
        Kind::AfterCall => format!("|receiver, args, original| {{ let value = original(receiver, args); {f}(receiver, &value); value }}"),
        Kind::ModifyValue => format!("|receiver, args, original| {{ let value = original(receiver, args); {f}(receiver, value) }}"),
        Kind::WrapCall => f.into(),
        Kind::WrapIf => format!("|receiver, args, original| {{ if {f}(receiver, &*args) {{ original(receiver, args); }} }}"),
        _ => unreachable!("a site kind"),
    }
}

#[cfg(test)]
mod tests;
