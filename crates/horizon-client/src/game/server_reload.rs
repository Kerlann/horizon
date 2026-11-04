//! Prepare server listeners against the new packs before publishing any data.
use mod_api::__private::game::*;

#[cfg(test)]
#[path = "server_reload_tests.rs"]
mod tests;
use super::packs::reload_files;
use super::{Game, functions::PackStack};
use horizon_client::datapack::DataStore;
pub(super) use mod_api::__private::reload::ReloadStageError;
use mod_api::{
    __private::reload::{apply_listeners, prepare_listeners},
    FileToIdConverter, Identifier, ServerReloadContext, ServerReloadListener,
    ServerReloadListeners, ServerReloadResources, VanillaServerListeners,
};

pub(super) type PreparedReload = PreparedStep<Box<DataStore>>;

use mod_api::__private::reload::PreparedStep;

/// Everything one successful reload publishes: its apply steps, then the
/// retained listeners' store.
pub(super) struct PreparedServerReload {
    pub(super) steps: Vec<PreparedReload>,
    pub(super) resources: ServerReloadResources,
}

/// Loads the vanilla function library from `stack`, logging per-file failures.
fn functions_store(stack: &PackStack) -> DataStore {
    let packs: Vec<_> = stack.builtin.iter().chain(stack.enabled_packs()).collect();
    let store = DataStore::load(&packs);
    for (id, e) in &store.errors {
        log::warn!("Horizon: failed to load function {id}: {e}");
    }
    store
}

impl PackStack {
    fn reload_context(
        &self,
        listener: &Identifier,
        converter: &FileToIdConverter,
    ) -> ServerReloadContext {
        let packs: Vec<_> = self.builtin.iter().chain(self.enabled_packs()).collect();
        ServerReloadContext::new(reload_files("data", &packs, listener, converter))
    }
}

impl Game<'_> {
    pub(super) fn prepare_server_reload(
        &self,
        stack: &PackStack,
    ) -> Result<PreparedServerReload, ReloadStageError> { if let Some(__l) = crate::points_generated::P5279.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        // Nobody listens: no registration graph, only the vanilla function library.
        if !mod_api::__private::listened(mod_api::HookId::AddServerReloadListeners) {
            return Ok(PreparedServerReload {
                steps: vec![PreparedStep::Vanilla(Box::new(functions_store(stack)))],
                resources: ServerReloadResources::default(),
            });
        }
        let mut listeners = ServerReloadListeners::new();
        self.native_add_server_reload_listeners(&mut listeners);
        let mut resources = ServerReloadResources::default();
        let ordered = listeners.finish().map_err(ReloadStageError::Order)?;
        resources.install(ordered.retained);
        let steps = prepare_listeners(
            ordered.listeners,
            "server_reload_prepare",
            |listener| {
                (listener.id == VanillaServerListeners::functions())
                    .then(|| Box::new(functions_store(stack)))
            },
            |id, converter| stack.reload_context(id, converter),
            ServerReloadListener::prepare,
        )?;
        Ok(PreparedServerReload { steps, resources })
    }
    pub(super) fn apply_server_reload(
        &mut self,
        prepared: PreparedServerReload,
        stack: &PackStack,
    ) { if let Some(__l) = crate::points_generated::P5278.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); }
        apply_listeners(prepared.steps, "server_reload_apply", |store| {
            self.fns.store = *store;
            self.sync_function_registries(stack);
        });
        self.reload_resources = prepared.resources;
    }
    /// The retained server reload listeners of the last successful reload.
    pub(super) fn native_reload_resources(&self) -> &ServerReloadResources { if let Some(__l) = crate::points_generated::P5277.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        &self.reload_resources
    }
}
