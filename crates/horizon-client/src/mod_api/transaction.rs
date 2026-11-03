//! `host.transaction` (#41 §7): NeoForge 26.3's `Transaction`. Changes made through the
//! capability helpers inside one are kept by the game and made final only when the outermost
//! transaction commits; an abort puts back everything its closure changed.
use super::Host;

/// What the game's raw transaction helpers take, and only `host.transaction`
/// makes: a mod opens and closes transactions through it alone, so its drop guard closes every
/// one a mod opens.
pub struct TransactionSeal(());

/// An open transaction, as NeoForge's `TransactionContext`: what every write of a handler or a
/// place takes, so none happens outside one. Only `host.transaction` makes one, and lends it to
/// its closure alone.
pub struct Transaction(());

impl dyn Host + '_ {
    /// Runs `f` in a transaction, nested in the open one if any (`Transaction.openRoot` and
    /// `open`): `Ok` commits it (`commit`), into the transaction around it or, at the
    /// outermost, for good, with the world told of each change once; `Err` aborts it
    /// (`close` without `commit`), putting every storage it wrote back as it was. A panic
    /// unwinding out of `f` aborts this transaction as it passes, so whoever catches it sees
    /// nothing `f` changed, and the transaction around it stays open.
    ///
    /// Covered: block entities (item slots, saved compound and attachments), cauldrons,
    /// players' inventories and cursors, entities' inventories
    /// ([`ItemPlace::EntitySlot`](super::ItemPlace::EntitySlot)) and item entities' stacks.
    pub fn transaction<T, E>(
        &mut self,
        f: impl FnOnce(&mut dyn Host, &Transaction) -> Result<T, E>,
    ) -> Result<T, E> {
        run_transaction(self, |host, tx| f(host, tx))
    }
}

/// `host.transaction` for a host of a known type, which `f` gets back: the game's own moves (a
/// hopper's) run in one through this.
pub fn run_transaction<H: Host + ?Sized, T, E>(
    host: &mut H,
    f: impl FnOnce(&mut H, &Transaction) -> Result<T, E>,
) -> Result<T, E> {
    /// Closes its transaction when dropped: committed when `f` gave `Ok`, else aborted, as on
    /// a panic unwinding past it.
    struct Open<'a, H: Host + ?Sized> {
        host: &'a mut H,
        commit: bool,
    }
    impl<H: Host + ?Sized> Drop for Open<'_, H> {
        fn drop(&mut self) {
            self.host
                .transaction_close(TransactionSeal(()), self.commit);
        }
    }
    host.transaction_open(TransactionSeal(()));
    let mut open = Open {
        host,
        commit: false,
    };
    let result = f(&mut *open.host, &Transaction(()));
    open.commit = result.is_ok();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records the transactions it is asked to open and close.
    #[derive(Default)]
    struct Journal {
        depth: usize,
        closed: Vec<(usize, bool)>,
    }
    impl Host for Journal {
        fn transaction_open(&mut self, _: TransactionSeal) {
            self.depth += 1;
        }
        fn transaction_close(&mut self, _: TransactionSeal, commit: bool) {
            self.closed.push((self.depth, commit));
            self.depth -= 1;
        }
    }

    #[test]
    fn a_transaction_commits_on_ok_and_aborts_on_err_and_nests() {
        let mut journal = Journal::default();
        let host: &mut dyn Host = &mut journal;
        let outer = host.transaction(|host, _| {
            let inner: Result<(), &str> = host.transaction(|_, _| Err("refused"));
            assert_eq!(inner, Err("refused"));
            host.transaction(|_, _| Ok::<_, ()>(7))
        });
        assert_eq!(outer, Ok(7));
        assert_eq!(journal.closed, [(2, false), (2, true), (1, true)]);
        assert_eq!(journal.depth, 0);
    }

    #[test]
    fn a_panic_aborts_exactly_the_transaction_it_leaves() {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        let mut journal = Journal::default();
        let host: &mut dyn Host = &mut journal;
        // A child's panic caught inside the root: the child aborts, the root still commits.
        let root = host.transaction(|host, _| {
            let child = catch_unwind(AssertUnwindSafe(|| {
                host.transaction(|_, _| -> Result<(), ()> { panic!("a broken handler") })
            }));
            assert!(child.is_err());
            Ok::<_, ()>(())
        });
        assert_eq!(root, Ok(()));
        // A root's panic caught outside it: the root aborts before the catch sees anything.
        let host: &mut dyn Host = &mut journal;
        let caught = catch_unwind(AssertUnwindSafe(|| {
            host.transaction(|_, _| -> Result<(), ()> { panic!("a broken handler") })
        }));
        assert!(caught.is_err());
        assert_eq!(journal.closed, [(2, false), (1, true), (1, false)]);
        assert_eq!(journal.depth, 0);
    }
}
