//! Read-only client session and local-player status declarations.

pub trait Helpers {
    /// The logged-in client's connected server. None outside a client world;
    /// missing display names and memory addresses remain explicit None fields.
    #[helper(
        side = "client",
        service = "client_session",
        target = "horizon_client::game::native_client_session::Game<'_>::native_client_server"
    )]
    fn client_server();
    /// The applied language code, not a pending options selection. None outside
    /// a logged-in client world. Read is_paused for the applied pause state.
    #[helper(
        side = "client",
        service = "client_session",
        target = "horizon_client::game::native_client_session::Game<'_>::native_client_language"
    )]
    fn client_language();
    /// This window's existing local frame counters; None outside a logged-in
    /// client world. No history is collected by this read and nothing is sent.
    #[helper(
        side = "client",
        service = "client_session",
        target = "horizon_client::game::native_client_session::Game<'_>::native_client_frame_stats"
    )]
    fn client_frame_stats();
    /// The local player's predicted block-breaking state. None for stale,
    /// nonlocal or fake handles and outside a logged-in client world; Idle
    /// means a valid local player is not currently breaking a block.
    #[helper(
        side = "client",
        receiver = "player",
        target = "horizon_client::game::native_client_session::Game<'_>::native_player_block_breaking"
    )]
    fn player_block_breaking();
    /// The local player's current cave ambience mood (normally 0 through 1),
    /// not a server value. None for stale, nonlocal or fake handles and outside
    /// a logged-in client world.
    #[helper(
        side = "client",
        receiver = "player",
        target = "horizon_client::game::native_client_session::Game<'_>::native_player_ambient_mood"
    )]
    fn player_ambient_mood();
}
