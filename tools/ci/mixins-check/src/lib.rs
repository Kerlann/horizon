//! A point listener, beside the helper injection in `count.rs`.
#[derive(Default)]
struct Check;

#[mod_api::horizon_mod]
impl mod_points::Points for Check {
    fn tick(&self, _: mod_api::HostSide<'_>, _: &mut mod_points::TickArgs) {}
}

#[mod_api::horizon_mod]
impl mod_api::Hooks for Check {}
