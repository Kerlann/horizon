mod api {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct BlockEntityTypeKey(pub u32);
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct BlockKey(pub u32);
}

use mod_api::mod_api::{BlockEntityTypeKey, BlockKey};

#[test]
fn block_entity_additions_collect_in_order_and_quiet_dispatch_preserves_input() {
    struct Listener(u32);
    #[mod_api::__subscriptions]
    impl Hooks for Listener {
        fn block_entity_type_add_blocks(
            &self,
            _: &RegistrationContext,
            event: &mut BlockEntityTypeAddBlocksEvent<'_>,
        ) {
            for &kind in event.types.iter().filter(|kind| kind.0 < 2) {
                event.additions.push((kind, BlockKey(self.0)));
            }
        }
    }
    let types = [
        BlockEntityTypeKey(0),
        BlockEntityTypeKey(1),
        BlockEntityTypeKey(2),
    ];
    let mut additions = vec![(types[2], BlockKey(8))];
    install(Vec::<ModEntry>::new().into());
    dispatch_block_entity_type_add_blocks(
        &RegistrationContext("bootstrap"),
        &types,
        &mut additions,
    );
    assert_eq!(additions, [(types[2], BlockKey(8))]);
    install(
        vec![
            ModEntry::new("first", Box::new(Listener(7))),
            ModEntry::new("second", Box::new(Listener(9))),
        ]
        .into(),
    );
    dispatch_block_entity_type_add_blocks(
        &RegistrationContext("bootstrap"),
        &types,
        &mut additions,
    );
    assert_eq!(
        additions,
        [
            (types[2], BlockKey(8)),
            (types[0], BlockKey(7)),
            (types[1], BlockKey(7)),
            (types[0], BlockKey(9)),
            (types[1], BlockKey(9))
        ]
    );
}
