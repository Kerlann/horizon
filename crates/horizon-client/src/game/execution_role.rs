//! A Game's execution role is fixed at construction. Connecting or disconnecting
//! changes `remote`, not this role. A client never simulates a world itself: single
//! player's runs on its integrated server thread.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExecutionRole {
    Client,
    IntegratedServer,
    DedicatedServer,
}

impl ExecutionRole {
    /// Only a client owns presentation work (audio, loading UI, chunk meshes).
    pub fn is_client(self) -> bool {
        self == Self::Client
    }

    /// Single-player worlds (on the integrated server thread) keep the owner's state in
    /// `Level` and write `players/data/<singleplayer_uuid>.dat` with the world; a dedicated
    /// server saves each connected player separately.
    pub fn writes_singleplayer_player(self) -> bool {
        self != Self::DedicatedServer
    }

    /// A logical server simulates the world: the integrated server thread or a
    /// dedicated server. A client — connected, or at a screen with no world —
    /// does not, whatever `remote` happens to say.
    pub fn is_logical_server(self) -> bool {
        self != Self::Client
    }
}

use super::Game;

impl Game<'_> {
    /// This game's side: a server simulates the world, a client does not. The
    /// one source for a helper's side; `remote` says whether a client is
    /// connected, not which side this game is.
    pub(super) fn is_logical_server(&self) -> bool {
        self.options.execution_role.is_logical_server()
    }
}
