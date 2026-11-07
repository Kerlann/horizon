//! Host method declarations; access defaults and game forwarders are generated.
#![allow(unused_variables)]
use crate::*;
/// The host's methods both sides share of the `mod_state` area, implemented by the game and reached through [`Host`].
#[horizon_mod_api_macros::host_area]
pub trait ModStateHost {
    /// Side: both.
    /// The server-owned holders' attachments; `TYPE.get(host, holder)` reads
    /// one. On a client it is the copy of what the server synced: `set` and
    /// `remove` change only the copy.
    #[access(WorldOnly)]
    fn attachments(&mut self) -> &mut dyn AttachmentStore;
}
/// The server host's methods of the `mod_state` area, implemented by the game and reached through [`ServerHost`].
#[horizon_mod_api_macros::host_area]
pub trait ModStateServerHost {
    /// Side: server.
    /// Opens a transaction, nested in the open one if any (`Transaction.open`). Only
    /// `host.transaction` reaches it, through its seal.
    #[access(WorldOnly)]
    fn transaction_open(&mut self, seal: TransactionSeal);
    /// Side: server.
    /// `native_transaction_close`, which only `host.transaction` reaches, through its seal.
    #[access(WorldOnly)]
    fn transaction_close(&mut self, seal: TransactionSeal, commit: bool);
}
