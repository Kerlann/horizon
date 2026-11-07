//! Experimental lazy variant only: not shipped as a second storage protocol.
use super::*;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

struct Lazy {
    entities: Vec<Entity>,
    slots: RefCell<HashMap<JoinSeq, usize>>,
    dirty: Cell<usize>,
}
impl Lazy {
    fn new(entities: Vec<Entity>) -> Self {
        let slots = entities
            .iter()
            .enumerate()
            .map(|(i, e)| (e.join, i))
            .collect();
        Self {
            entities,
            slots: RefCell::new(slots),
            dirty: Cell::new(usize::MAX),
        }
    }
    fn remove(&mut self, index: usize) {
        let entity = self.entities.remove(index);
        self.slots.get_mut().remove(&entity.join);
        self.dirty.set(self.dirty.get().min(index));
    }
    fn lookup(&self, join: JoinSeq) -> Option<usize> {
        let dirty = self.dirty.replace(usize::MAX);
        let mut slots = self.slots.borrow_mut();
        if dirty != usize::MAX {
            for (i, entity) in self.entities.iter().enumerate().skip(dirty) {
                slots.insert(entity.join, i);
            }
        }
        slots.get(&join).copied()
    }
}

pub fn run() {
    println!("mixed,N,removals,schedule,eager ns,lazy ns,Vec ns");
    for n in [1_000, 10_000] {
        let template: JoinList<_> = (0..n)
            .map(|_| Entity {
                join: JoinSeq::UNJOINED,
                payload: [0; 7],
            })
            .collect();
        for removals in [1, 10, 100] {
            for interleaved in [false, true] {
                let mut totals = [0.0; 3];
                // Exactly N lookups of surviving entities per tick. Middle removals.
                // Burst is the coalescing best case; interleaved repairs each time.
                for _ in 0..20 {
                    let mut eager = template.clone();
                    let mut lazy = Lazy::new(template.to_vec());
                    let mut plain = template.to_vec();
                    let joins: Vec<_> = template[..n / 4].iter().map(|e| e.join).collect();
                    totals[0] += time(
                        || {
                            for r in 0..removals {
                                black_box(eager.remove(eager.len() / 2));
                                if interleaved {
                                    for i in 0..n / removals {
                                        black_box(
                                            eager.join_slot(joins[(i + r * 97) % joins.len()]),
                                        );
                                    }
                                }
                            }
                            if !interleaved {
                                for i in 0..n {
                                    black_box(eager.join_slot(joins[i % joins.len()]));
                                }
                            }
                        },
                        1,
                    )
                    .0;
                    totals[1] += time(
                        || {
                            for r in 0..removals {
                                lazy.remove(lazy.entities.len() / 2);
                                if interleaved {
                                    for i in 0..n / removals {
                                        black_box(lazy.lookup(joins[(i + r * 97) % joins.len()]));
                                    }
                                }
                            }
                            if !interleaved {
                                for i in 0..n {
                                    black_box(lazy.lookup(joins[i % joins.len()]));
                                }
                            }
                        },
                        1,
                    )
                    .0;
                    totals[2] += time(
                        || {
                            for r in 0..removals {
                                black_box(plain.remove(plain.len() / 2));
                                if interleaved {
                                    for i in 0..n / removals {
                                        black_box(old_find(
                                            &plain,
                                            joins[(i + r * 97) % joins.len()],
                                        ));
                                    }
                                }
                            }
                            if !interleaved {
                                for i in 0..n {
                                    black_box(old_find(&plain, joins[i % joins.len()]));
                                }
                            }
                        },
                        1,
                    )
                    .0;
                    for &join in &joins {
                        assert_eq!(lazy.lookup(join), old_find(&plain, join));
                    }
                }
                println!(
                    "mixed,{n},{removals},{},{:.0},{:.0},{:.0}",
                    if interleaved { "interleaved" } else { "burst" },
                    totals[0] / 20.0,
                    totals[1] / 20.0,
                    totals[2] / 20.0
                );
            }
        }
    }
}
