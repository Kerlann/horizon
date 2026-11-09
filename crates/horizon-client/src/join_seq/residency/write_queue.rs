//! Owned after-writer commands, independent of Game and callback dispatch.
//!
//! The caller validates authority, arguments and domain budgets before acceptance.
//! It supplies the enclosing writer turn even from nested callbacks, and drains
//! only after writer store-back and before the next actor. Never hold a queue
//! borrow during apply or event dispatch. Publish authoritative rows before
//! dispatching the returned completion to its issuer. Apply must revalidate the
//! captured runtime id AND location generation; this core never resolves targets.
use super::table::Location;
use horizon_core::fast_hash::FxHashMap;
use mod_api::{CommandId, WorldEpoch};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Issuer(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriterTurn(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transaction(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub runtime_id: u32,
    pub location: Location,
}

/// An owned operation, never an index or borrowed callback.
pub struct Command<O> {
    pub id: CommandId,
    pub issuer: Issuer,
    pub turn: WriterTurn,
    pub target: Target,
    pub operation: O,
    transaction: Option<Transaction>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Query<'a, C> {
    Pending,
    Terminal(&'a C),
}
#[derive(Debug, PartialEq, Eq)]
pub enum QueryError {
    StaleEpoch,
    UnknownOrExpired,
}
struct Record<C> {
    issuer: Issuer,
    terminal: Option<C>,
}

/// One queue per world session, shared across all targets and issuers.
/// C is the caller's operation-tagged terminal sum preserving each real R/E.
/// Pending is never evicted, and no bool/count is synthesized on acceptance.
pub struct WriteQueue<O, C> {
    epoch: WorldEpoch,
    next: Option<u64>,
    pending: VecDeque<Command<O>>,
    records: FxHashMap<CommandId, Record<C>>,
    applying: Option<CommandId>,
    tick: u64,
    expiry: VecDeque<(u64, CommandId)>,
}
impl<O, C> WriteQueue<O, C> {
    pub fn new(epoch: WorldEpoch) -> Self {
        Self {
            epoch,
            next: Some(0),
            pending: VecDeque::new(),
            records: FxHashMap::default(),
            applying: None,
            tick: 0,
            expiry: VecDeque::new(),
        }
    }

    /// Exhaustion returns the owned operation; no id is reused within an epoch.
    pub fn accept(
        &mut self,
        issuer: Issuer,
        turn: WriterTurn,
        target: Target,
        transaction: Option<Transaction>,
        operation: O,
    ) -> Result<CommandId, O> {
        let Some(sequence) = self.next else {
            return Err(operation);
        };
        self.next = sequence.checked_add(1);
        let id = CommandId {
            epoch: self.epoch,
            sequence,
        };
        self.records.insert(
            id,
            Record {
                issuer,
                terminal: None,
            },
        );
        self.pending.push_back(Command {
            id,
            issuer,
            turn,
            target,
            operation,
            transaction,
        });
        Ok(id)
    }

    /// The caller certifies this turn has stored back. Only one apply may run;
    /// recursive drains from callbacks cannot overtake a half-applied command.
    pub fn take_next(&mut self, stored_turn: WriterTurn) -> Option<Command<O>> {
        if self.applying.is_some() || self.pending.front()?.turn != stored_turn {
            return None;
        }
        let command = self.pending.pop_front()?;
        self.applying = Some(command.id);
        Some(command)
    }

    /// Returns the exact retained value for event delivery, after row publication.
    /// A stale, duplicated or out-of-order completion returns its value unchanged.
    pub fn complete(&mut self, id: CommandId, completion: C) -> Result<&C, C> {
        if self.applying != Some(id) {
            return Err(completion);
        }
        let record = self
            .records
            .get_mut(&id)
            .expect("applying command has a record");
        record.terminal = Some(completion);
        self.applying = None;
        self.expiry.push_back((self.tick.saturating_add(1), id));
        Ok(record.terminal.as_ref().expect("just completed"))
    }

    pub fn query(&self, issuer: Issuer, id: CommandId) -> Result<Query<'_, C>, QueryError> {
        if id.epoch != self.epoch {
            return Err(QueryError::StaleEpoch);
        }
        let record = self
            .records
            .get(&id)
            .filter(|r| r.issuer == issuer)
            .ok_or(QueryError::UnknownOrExpired)?;
        Ok(match &record.terminal {
            Some(value) => Query::Terminal(value),
            None => Query::Pending,
        })
    }

    /// Roll back before drain (transactions must finish before writer store-back).
    /// Removed ids let the caller remove overlays; no fake apply event is emitted.
    pub fn rollback(&mut self, transaction: Transaction) -> Vec<CommandId> {
        let mut removed = Vec::new();
        self.pending.retain(|command| {
            if command.transaction == Some(transaction) {
                removed.push(command.id);
                false
            } else {
                true
            }
        });
        for id in &removed {
            self.records.remove(id);
        }
        removed
    }

    /// Called once at the END of each logical tick, after completion events.
    /// A terminal completed in tick N survives through the end of tick N+1.
    pub fn end_tick(&mut self) {
        while self
            .expiry
            .front()
            .is_some_and(|(end, _)| *end <= self.tick)
        {
            let (_, id) = self.expiry.pop_front().expect("front exists");
            self.records.remove(&id);
        }
        self.tick = self.tick.checked_add(1).expect("logical tick exhausted");
    }

    /// Epochs must strictly increase, preventing old ids from ever aliasing.
    pub fn reset(&mut self, epoch: WorldEpoch) {
        assert!(epoch.get() > self.epoch.get(), "world epochs must increase");
        *self = Self::new(epoch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::join_seq::{
        JoinSeq,
        residency::{Kind, Owner},
    };
    use mod_api::DimensionKey;
    fn target(id: u32) -> Target {
        Target {
            runtime_id: id,
            location: Location {
                source: super::super::Source::Ordinary,
                owner: Owner {
                    dimension: DimensionKey::OVERWORLD,
                    kind: Kind::Mob,
                },
                join: JoinSeq::next(),
            },
        }
    }
    #[test]
    fn ordered_apply_callbacks_and_terminal_lifecycle() {
        let mut queue = WriteQueue::<i32, Result<i32, &'static str>>::new(WorldEpoch::new(1));
        let issuer = Issuer(4);
        let turn = WriterTurn(1);
        let a = target(1);
        let b = target(2);
        let first = queue.accept(issuer, turn, a, None, 2).unwrap();
        let second = queue.accept(issuer, turn, b, None, 3).unwrap();
        assert_eq!(queue.query(issuer, first), Ok(Query::Pending));
        assert_eq!(
            queue.query(Issuer(5), first),
            Err(QueryError::UnknownOrExpired)
        );
        assert!(queue.take_next(WriterTurn(2)).is_none());
        let mut state = 1;
        let mut events = Vec::new();
        let mut apply = |queue: &mut WriteQueue<i32, Result<i32, &'static str>>,
                         command: Command<i32>| {
            state = state * 10 + command.operation;
            if command.id == first {
                assert_eq!(command.target, a);
                // An apply callback can append, but cannot drain recursively.
                queue.accept(issuer, turn, b, None, 4).unwrap();
                assert!(queue.take_next(turn).is_none());
            }
            (command.id, Ok(state))
        };
        while let Some(command) = queue.take_next(turn) {
            let (id, completion) = apply(&mut queue, command);
            let event = *queue.complete(id, completion).unwrap();
            assert_eq!(queue.query(issuer, id), Ok(Query::Terminal(&event)));
            events.push(event);
        }
        assert_eq!(events, [Ok(12), Ok(123), Ok(1234)]);
        assert_eq!(
            queue.complete(second, Err("duplicate")),
            Err(Err("duplicate"))
        );
        let pending = queue.accept(issuer, turn, a, None, 9).unwrap();
        queue.end_tick();
        assert_eq!(queue.query(issuer, first), Ok(Query::Terminal(&Ok(12))));
        queue.end_tick();
        assert_eq!(
            queue.query(issuer, first),
            Err(QueryError::UnknownOrExpired)
        );
        assert_eq!(queue.query(issuer, pending), Ok(Query::Pending));
        queue.reset(WorldEpoch::new(2));
        assert_eq!(queue.query(issuer, pending), Err(QueryError::StaleEpoch));
    }
    #[test]
    fn rollback_preserves_survivor_order_and_generation() {
        let mut queue = WriteQueue::<i32, i32>::new(WorldEpoch::new(1));
        let old = target(7);
        let replacement = target(7);
        assert_ne!(old.location, replacement.location);
        let mut ids = Vec::new();
        for n in 0..4 {
            ids.push(
                queue
                    .accept(
                        Issuer(0),
                        WriterTurn(0),
                        old,
                        (n % 2 == 0).then_some(Transaction(8)),
                        n,
                    )
                    .unwrap(),
            );
        }
        assert_eq!(queue.rollback(Transaction(8)), [ids[0], ids[2]]);
        let mut state = Vec::new();
        while let Some(command) = queue.take_next(WriterTurn(0)) {
            assert_eq!(command.target, old);
            assert_ne!(command.target.location, replacement.location);
            state.push(command.operation);
            queue.complete(command.id, command.operation).unwrap();
        }
        assert_eq!(state, [1, 3]);
        assert_eq!(
            queue.query(Issuer(0), ids[0]),
            Err(QueryError::UnknownOrExpired)
        );
        queue.next = Some(u64::MAX);
        queue
            .accept(Issuer(0), WriterTurn(0), old, None, 8)
            .unwrap();
        assert_eq!(queue.accept(Issuer(0), WriterTurn(0), old, None, 9), Err(9));
    }
}
