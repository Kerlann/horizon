//! Issuer-only terminal delivery for accepted entity work.
pub trait Hooks {
    /// An accepted entity command finished after authoritative application or
    /// typed refusal and publication. Only the issuing mod/package receives
    /// this event. `completion` is exactly the terminal value returned by its
    /// CommandId query, retained through the end of the following logical tick.
    /// The enclosing writer turn remains active: foreign writes made here append
    /// to its FIFO and never recursively drain. OutsideScope or unknown query
    /// results must not be interpreted as successful application.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (Horizon-only queued entity command receipt)"
    )]
    fn entity_command_completed();
}
