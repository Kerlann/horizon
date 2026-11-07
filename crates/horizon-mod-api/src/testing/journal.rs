//! The transaction journal fake: what a host keeps on each transaction it opens.

/// A test host's transaction journal, as the game's keeps its storages: the state each open
/// transaction began with, put back when it aborts, and every close in order.
///
/// [`FakeHost`](super::FakeHost) keeps one over its players and level. A test host of its own
/// keeps one over its state: it calls [`open`](Self::open) from `transaction_open`, and puts back
/// what [`close`](Self::close) returns from `transaction_close`.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct FakeJournal<S> {
    kept: Vec<S>,
    closed: Vec<(usize, bool)>,
}

impl<S> Default for FakeJournal<S> {
    fn default() -> Self {
        Self {
            kept: Vec::new(),
            closed: Vec::new(),
        }
    }
}

impl<S> FakeJournal<S> {
    /// Opens a transaction, nested in the open one if any, that began with state `now`.
    pub fn open(&mut self, now: S) {
        self.kept.push(now);
    }
    /// Closes the innermost open transaction: committed, or aborted, when it gives back the
    /// state it began with for the host to put back. Nothing when none is open.
    pub fn close(&mut self, commit: bool) -> Option<S> {
        let was = self.kept.pop()?;
        self.closed.push((self.kept.len() + 1, commit));
        (!commit).then_some(was)
    }
    /// How many transactions are open.
    pub fn depth(&self) -> usize {
        self.kept.len()
    }
    /// Each transaction closed, in order: the depth it was open at (1 for the outermost), and
    /// whether it committed.
    pub fn closed(&self) -> &[(usize, bool)] {
        &self.closed
    }
}
