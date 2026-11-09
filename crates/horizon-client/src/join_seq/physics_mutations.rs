//! Scoped identity deltas for physics indexes, independent of residency.
//! No element address or position is retained; consumers resolve current joins.
use super::{JoinSeq, Joined};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Touched,
    Added,
    Removed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mutation {
    pub join: JoinSeq,
    pub change: Change,
}
#[derive(Debug)]
struct State {
    active: AtomicBool,
    events: Mutex<Vec<Mutation>>,
}
#[derive(Debug)]
struct Node {
    state: Weak<State>,
    previous: Option<Arc<Node>>,
}

/// A collector owns no entity storage. Dropping it only closes its own sink.
#[derive(Debug)]
pub struct Watch {
    state: Arc<State>,
}
impl Watch {
    /// Swap reusable buffers, releasing the collector lock before processing.
    pub fn drain(&self, output: &mut Vec<Mutation>) {
        output.clear();
        std::mem::swap(output, &mut self.state.events.lock().unwrap());
    }
}
impl Drop for Watch {
    fn drop(&mut self) {
        self.state.active.store(false, Ordering::Release);
        self.state.events.lock().unwrap().clear();
    }
}

#[derive(Debug)]
pub(super) struct Binding<T> {
    identify: fn(&T) -> JoinSeq,
    node: Arc<Node>,
}
impl<T> Clone for Binding<T> {
    fn clone(&self) -> Self {
        Self {
            identify: self.identify,
            node: self.node.clone(),
        }
    }
}
impl<T> Binding<T> {
    pub(super) fn start(previous: Option<&Self>, capacity: usize) -> (Self, Watch)
    where
        T: Joined,
    {
        let mut previous = previous.map(|binding| binding.node.clone());
        while let Some(node) = &previous {
            if node
                .state
                .upgrade()
                .is_some_and(|state| state.active.load(Ordering::Acquire))
            {
                break;
            }
            previous = node.previous.clone();
        }
        let state = Arc::new(State {
            active: AtomicBool::new(true),
            events: Mutex::new(Vec::with_capacity(capacity)),
        });
        (
            Self {
                identify: Joined::join_seq,
                node: Arc::new(Node {
                    state: Arc::downgrade(&state),
                    previous,
                }),
            },
            Watch { state },
        )
    }
    pub(super) fn same(&self, other: &Self) -> bool {
        self.covers(other) || other.covers(self)
    }
    pub(super) fn covers(&self, other: &Self) -> bool {
        let mut node = Some(&self.node);
        while let Some(at) = node {
            if Arc::ptr_eq(at, &other.node) {
                return true;
            }
            node = at.previous.as_ref();
        }
        false
    }
    pub(super) fn active(&self) -> bool {
        let mut node = Some(&*self.node);
        while let Some(at) = node {
            if at
                .state
                .upgrade()
                .is_some_and(|state| state.active.load(Ordering::Acquire))
            {
                return true;
            }
            node = at.previous.as_deref();
        }
        false
    }
    pub(super) fn record(&self, entity: &T, change: Change) {
        if !self.active() {
            return;
        }
        let mutation = Mutation {
            join: (self.identify)(entity),
            change,
        };
        let mut node = Some(&*self.node);
        while let Some(at) = node {
            if let Some(state) = at.state.upgrade() {
                let mut events = state.events.lock().unwrap();
                if state.active.load(Ordering::Acquire) {
                    events.push(mutation);
                }
            }
            node = at.previous.as_deref();
        }
    }
    pub(super) fn all(&self, entities: &[T], change: Change) {
        if self.active() {
            for entity in entities {
                self.record(entity, change);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::join_seq::{JoinList, RestoreOrder, residency};
    #[derive(Clone, Debug)]
    struct Item {
        join: JoinSeq,
        id: u32,
        data: u32,
    }
    crate::impl_joined!(Item);
    fn item(id: u32) -> Item {
        Item {
            join: JoinSeq::UNJOINED,
            id,
            data: 0,
        }
    }

    #[test]
    fn exact_joins_survive_compaction_and_physical_lifts_without_admission_events() {
        let mut items: JoinList<_> = [item(1), item(2), item(3)].into_iter().collect();
        let joins: Vec<_> = items.iter().map(|item| item.join).collect();
        let journal = residency::Journal::default();
        items.bind_residency(
            &journal,
            residency::Owner {
                dimension: mod_api::DimensionKey::OVERWORLD,
                kind: residency::Kind::Item,
            },
            |item| (item.id, item.join),
        );
        let mut admitted = Vec::new();
        journal.drain_into(&mut admitted);
        let watch = items.watch_physics_mutations();
        let mut changes = Vec::new();
        let mut taken = items.lift();
        taken.update(2, |item| item.data = 9);
        items.restore(taken, RestoreOrder::SurvivorsFirst);
        watch.drain(&mut changes);
        assert_eq!(
            changes,
            [Mutation {
                join: joins[2],
                change: Change::Touched
            }]
        );
        admitted.clear();
        journal.drain_into(&mut admitted);
        assert_eq!(
            admitted
                .iter()
                .map(|event| event.change)
                .collect::<Vec<_>>(),
            [residency::Change::Lifted, residency::Change::Restored]
        );
        items.remove(0);
        items.update(1, |item| item.data = 12);
        items.push(item(4));
        let fresh = items.last().unwrap().join;
        watch.drain(&mut changes);
        assert_eq!(
            changes,
            [
                Mutation {
                    join: joins[0],
                    change: Change::Removed
                },
                Mutation {
                    join: joins[2],
                    change: Change::Touched
                },
                Mutation {
                    join: fresh,
                    change: Change::Added
                }
            ]
        );
        assert_eq!(
            items
                .iter()
                .map(|item| (item.id, item.data))
                .collect::<Vec<_>>(),
            [(2, 0), (3, 12), (4, 0)]
        );
    }

    #[test]
    fn nested_watch_closure_and_unwind_do_not_close_outer_collector() {
        let mut items: JoinList<_> = [item(1), item(2)].into_iter().collect();
        let join = items[1].join;
        let outer = items.watch_physics_mutations();
        let mut changes = Vec::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _inner = items.watch_physics_mutations();
            items.update(1, |item| item.data = 7);
            panic!("physics callback");
        }));
        assert!(result.is_err());
        items.update(1, |item| item.data = 8);
        outer.drain(&mut changes);
        assert_eq!(
            changes,
            [Mutation {
                join,
                change: Change::Touched
            }; 2]
        );
        assert_eq!(items[1].data, 8);
        let closed = Arc::downgrade(&outer.state);
        drop(outer);
        items.update(0, |item| item.data = 3);
        assert!(closed.upgrade().is_none());
    }

    #[test]
    fn disjoint_mutation_and_lazy_iteration_watch_only_lent_references() {
        let mut items: JoinList<_> = (1..=5).map(item).collect();
        let joins: Vec<_> = items.iter().map(|item| item.join).collect();
        let watch = items.watch_physics_mutations();
        let mut changes = Vec::new();
        items.iter_mut().nth(3).unwrap().data = 4;
        watch.drain(&mut changes);
        assert_eq!(
            changes,
            [Mutation {
                join: joins[3],
                change: Change::Touched
            }]
        );
        items.iter_mut().find(|item| item.id == 5).unwrap().data = 9;
        watch.drain(&mut changes);
        assert_eq!(
            changes,
            [Mutation {
                join: joins[4],
                change: Change::Touched
            }]
        );
        items.with_two_and_rest(4, 1, |a, b, _, _, _| {
            a.data = 5;
            b.data = 2;
        });
        watch.drain(&mut changes);
        assert_eq!(
            changes,
            [
                Mutation {
                    join: joins[4],
                    change: Change::Touched
                },
                Mutation {
                    join: joins[1],
                    change: Change::Touched
                }
            ]
        );
        assert_eq!(
            items.iter().map(|item| item.data).collect::<Vec<_>>(),
            [0, 2, 0, 4, 5]
        );
    }
    #[test]
    fn readonly_predicates_and_nested_physical_restore_do_not_dirty_the_population() {
        let mut items: JoinList<_> = (1..=4).map(item).collect();
        let outer = items.watch_physics_mutations();
        let mut changes = Vec::new();
        items.retain(|_| true);
        assert_eq!(items.extract_if(|_| false).count(), 0);
        outer.drain(&mut changes);
        assert!(changes.is_empty());
        let mut taken = items.lift();
        let inner = items.watch_physics_mutations();
        let join = taken[2].join;
        taken.update(2, |item| item.data = 6);
        items.restore(taken, RestoreOrder::SurvivorsFirst);
        outer.drain(&mut changes);
        assert_eq!(
            changes,
            [Mutation {
                join,
                change: Change::Touched
            }]
        );
        inner.drain(&mut changes);
        assert!(changes.is_empty());
        drop(inner);
        let mut body = items.extract(join).unwrap();
        body.data = 8;
        items.put_back(2, body);
        outer.drain(&mut changes);
        assert_eq!(
            changes,
            [Mutation {
                join,
                change: Change::Touched
            }; 2]
        );
        assert_eq!(items[2].data, 8);
    }
}
