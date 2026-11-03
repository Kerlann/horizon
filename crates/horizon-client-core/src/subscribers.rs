//! Subscriber sets for generated native-hook dispatch.
use std::{
    any::{Any, TypeId},
    cell::Cell,
    marker::PhantomData,
    rc::Rc,
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
};

#[derive(Clone, Copy)]
struct Active {
    subscribers: &'static Subscribers,
    hook: usize,
    index: usize,
    receiver: *const (),
    receiver_type: TypeId,
}

thread_local! {
    static ACTIVE: Cell<Option<Active>> = const { Cell::new(None) };
}

/// Restores the previous invocation, including after a nested dispatch.
pub struct Entered(Option<Active>, PhantomData<Rc<()>>);

impl Drop for Entered {
    fn drop(&mut self) {
        ACTIVE.set(self.0);
    }
}

/// Mark a subscriber as the receiver of the current dispatch call.
pub fn enter(
    subscribers: &'static Subscribers,
    hook: usize,
    index: usize,
    receiver: *const (),
    receiver_type: TypeId,
) -> Entered {
    let active = Active {
        subscribers,
        hook,
        index,
        receiver,
        receiver_type,
    };
    Entered(ACTIVE.replace(Some(active)), PhantomData)
}

/// Report a generated default only for the exact receiver in this dispatch.
pub fn default_called<T: Any + ?Sized>(domain: Domain, hook: usize, receiver: &T) {
    if let Some(active) = ACTIVE.get()
        && active.subscribers.domain == domain
        && active.hook == hook
        && active.receiver == std::ptr::from_ref(receiver).cast::<()>()
        && active.receiver_type == TypeId::of::<T>()
    {
        active.subscribers.remove(hook, active.index);
    }
}

/// Which generated hook table an id belongs to: gameplay and render ids both start at 0.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Domain {
    Gameplay,
    Render,
}

/// Per-hook sets of subscribers, initially containing every installed mod.
pub struct Subscribers {
    domain: Domain,
    words_per_hook: usize,
    words: Box<[AtomicU64]>,
    counts: Box<[AtomicUsize]>,
}

impl Subscribers {
    /// Allocate the sets once, at installation.
    pub fn new(domain: Domain, hooks: usize, mods: usize) -> Self {
        let words_per_hook = mods.div_ceil(64);
        let mut words = Vec::with_capacity(hooks * words_per_hook);
        for _ in 0..hooks {
            for word in 0..words_per_hook {
                let remaining = mods - word * 64;
                let bits = if remaining >= 64 {
                    u64::MAX
                } else {
                    (1u64 << remaining) - 1
                };
                words.push(AtomicU64::new(bits));
            }
        }
        Self {
            domain,
            words_per_hook,
            words: words.into_boxed_slice(),
            counts: (0..hooks).map(|_| AtomicUsize::new(mods)).collect(),
        }
    }

    /// Whether a hook has a subscriber. Once false, this stays false.
    pub fn contains_any(&self, hook: usize) -> bool {
        self.counts[hook].load(Ordering::Relaxed) != 0
    }

    /// Remove a mod that ran the generated default, at most once under races.
    pub fn remove(&self, hook: usize, index: usize) {
        let word = &self.words[hook * self.words_per_hook + index / 64];
        let bit = 1u64 << (index % 64);
        if word.fetch_and(!bit, Ordering::Relaxed) & bit != 0 {
            self.counts[hook].fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Visit the remaining subscribers in installation order, without allocating.
    pub fn indices(&self, hook: usize) -> Indices<'_> {
        let start = hook * self.words_per_hook;
        Indices {
            words: &self.words[start..start + self.words_per_hook],
            next_word: 0,
            bits: 0,
        }
    }
}

/// Iterator over the set bits of one hook, in mod order.
pub struct Indices<'a> {
    words: &'a [AtomicU64],
    next_word: usize,
    bits: u64,
}

impl Iterator for Indices<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        while self.bits == 0 {
            self.bits = self.words.get(self.next_word)?.load(Ordering::Relaxed);
            self.next_word += 1;
        }
        let bit = self.bits.trailing_zeros() as usize;
        self.bits &= self.bits - 1;
        Some((self.next_word - 1) * 64 + bit)
    }
}

#[cfg(test)]
mod tests {
    use super::{Domain, Subscribers, default_called, enter};
    use std::any::TypeId;

    #[test]
    fn guards_restore_nested_calls_and_distinguish_zero_sized_types() {
        struct A;
        struct B;
        let subscribers = Box::leak(Box::new(Subscribers::new(Domain::Gameplay, 2, 2)));
        let a = A;
        let b = B;
        let pointer = std::ptr::from_ref(&a).cast();
        let outer = enter(subscribers, 0, 0, pointer, TypeId::of::<A>());
        // Even coincident zero-sized addresses do not identify another type as A.
        default_called(Domain::Gameplay, 0, &b);
        assert!(subscribers.indices(0).any(|i| i == 0));
        {
            let _inner = enter(
                subscribers,
                1,
                1,
                std::ptr::from_ref(&b).cast(),
                TypeId::of::<B>(),
            );
            default_called(Domain::Gameplay, 1, &b);
        }
        default_called(Domain::Gameplay, 0, &a);
        drop(outer);
        assert_eq!(subscribers.indices(0).collect::<Vec<_>>(), [1]);
        assert_eq!(subscribers.indices(1).collect::<Vec<_>>(), [0]);
    }

    /// A default from the other table, with the same id and receiver, prunes nothing.
    #[test]
    fn guards_distinguish_domains_with_equal_ids() {
        struct Both;
        let subscribers = Box::leak(Box::new(Subscribers::new(Domain::Render, 2, 1)));
        let both = Both;
        let guard = enter(
            subscribers,
            1,
            0,
            std::ptr::from_ref(&both).cast(),
            TypeId::of::<Both>(),
        );
        default_called(Domain::Gameplay, 1, &both);
        assert!(subscribers.contains_any(1));
        default_called(Domain::Render, 1, &both);
        drop(guard);
        assert!(!subscribers.contains_any(1));
    }

    #[test]
    fn guards_are_thread_local() {
        let subscribers = Box::leak(Box::new(Subscribers::new(Domain::Gameplay, 1, 2)));
        let subscribers: &'static Subscribers = subscribers;
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            for index in 0..2 {
                let barrier = &barrier;
                scope.spawn(move || {
                    let receiver = index;
                    let _guard = enter(
                        subscribers,
                        0,
                        index,
                        std::ptr::from_ref(&receiver).cast(),
                        TypeId::of::<usize>(),
                    );
                    barrier.wait();
                    default_called(Domain::Gameplay, 0, &receiver);
                });
            }
        });
        assert!(!subscribers.contains_any(0));
    }

    #[test]
    fn count_tracks_unique_removals_across_words() {
        let subscribers = Subscribers::new(Domain::Gameplay, 1, 70);
        std::thread::scope(|scope| {
            for _ in 0..2 {
                scope.spawn(|| {
                    for i in 0..70 {
                        if i != 66 {
                            subscribers.remove(0, i);
                        }
                    }
                });
            }
        });
        assert!(subscribers.contains_any(0));
        assert_eq!(subscribers.indices(0).collect::<Vec<_>>(), [66]);
        subscribers.remove(0, 66);
        assert!(!subscribers.contains_any(0));
        assert_eq!(subscribers.indices(0).next(), None);
    }
}
