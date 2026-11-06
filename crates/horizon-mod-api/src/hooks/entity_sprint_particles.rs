//! Client-side entity sprint-particle helper.

pub trait Helpers {
    #[helper(
        side = "client",
        receiver = "entity",
        target = "horizon_client::game::client_ambient::Game<'_>::native_entity_sprint_particles"
    )]
    fn entity_sprint_particles();
}
