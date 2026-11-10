"""Mechanical public receipt/caller migration for the NoGravity scalar domain.

Run before the operation-specific implementation; generated declarations remain
owned by generate_entity_commands.py and per-operation JSON.
"""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def replace(path, old, new):
    text = path.read_text()
    if text.count(old) != 1:
        raise ValueError(f"{path}: expected exactly one migration target")
    path.write_text(text.replace(old, new))


def run():
    replace(
        ROOT / "crates/horizon-mod-api/src/host/entities.rs",
        """    /// Sets the mob's no-gravity flag (`Entity.setNoGravity`). Returns false for players and guests.
    #[access(WorldOnly)]
    fn entity_set_no_gravity(&mut self, entity: EntityRef, no_gravity: bool) -> bool;""",
        """    /// Sets the stored no-gravity flag (`Entity.setNoGravity`).
    /// Direct and explicit OWN calls apply immediately. Foreign calls during a
    /// writer turn return Pending and execute in FIFO order after that writer.
    /// Pending light reads project the latest ordered value without claiming
    /// authoritative application. Completion event and issuer-only query expose
    /// the actual result after generation/removal checks. An item transaction
    /// refuses an enqueue with TransactionActive before creating an id/overlay.
    /// A family without implemented storage refuses Unsupported; this does not
    /// imply that vanilla forbids no-gravity for that entity kind.
    #[access(WorldOnly)]
    fn entity_set_no_gravity(
        &mut self,
        entity: EntityRef,
        no_gravity: bool,
    ) -> WriteReceipt<bool, EntityWriteError>;""",
    )
    path = ROOT / "crates/horizon-client/src/game/modding/native_entity_state.rs"
    replace(path,
            "assert!(entity.set_no_gravity(game, true));",
            "assert_eq!(entity.set_no_gravity(game, true), mod_api::WriteReceipt::Applied(true));")
    replace(path,
            "with_player(|game, player| assert!(!player.set_no_gravity(game, true)));",
            "with_player(|game, player| assert_eq!(player.set_no_gravity(game, true), mod_api::WriteReceipt::Refused(mod_api::EntityWriteError::Unsupported)));")


if __name__ == "__main__":
    run()
