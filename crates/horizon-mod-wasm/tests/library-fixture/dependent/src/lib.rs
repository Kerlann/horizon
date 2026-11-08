use mod_api::Hooks;

#[derive(Default)]
pub struct Mod;

#[mod_api::horizon_mod]
impl Hooks for Mod {
    fn register(&self, registrar: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
        let kind = registrar.block_entity_type("dependent").blocks(&[]).build();
        registrar.block_entity_part(&kind, Dependent).unwrap();
    }
}

struct Dependent;
#[mod_api::horizon_part]
impl mod_api::BlockPart for Dependent {}
#[mod_api::horizon_part]
impl mod_api::BlockEntityPart for Dependent {
    fn block_entity_tick(
        &self,
        _: &mut dyn mod_api::ServerHost,
        event: &mut mod_api::BlockEntityTickEvent<'_>,
    ) {
        event.entity.data = vec![wasm_test_library::shared_value()];
    }
}
