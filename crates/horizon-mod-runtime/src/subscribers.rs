//! Subscriber sets for generated native-hook dispatch.

/// Immutable per-hook sets of subscribers, resolved once at installation.
pub struct Subscribers {
    words_per_hook: usize,
    words: Box<[u64]>,
    active: Box<[bool]>,
}

impl Subscribers {
    /// Build the sets from `(mod index, hook index)` pairs.
    pub fn new(
        hooks: usize,
        mods: usize,
        subscriptions: impl IntoIterator<Item = (usize, usize)>,
    ) -> Self {
        let words_per_hook = mods.div_ceil(64);
        let mut words = vec![0; hooks * words_per_hook];
        let mut active = vec![false; hooks];
        for (index, hook) in subscriptions {
            assert!(index < mods && hook < hooks);
            words[hook * words_per_hook + index / 64] |= 1 << (index % 64);
            active[hook] = true;
        }
        Self {
            words_per_hook,
            words: words.into_boxed_slice(),
            active: active.into_boxed_slice(),
        }
    }

    /// Whether a hook has a subscriber.
    pub fn contains_any(&self, hook: usize) -> bool {
        self.active[hook]
    }

    /// Whether a particular mod subscribes to a hook.
    pub fn contains(&self, hook: usize, index: usize) -> bool {
        self.words[hook * self.words_per_hook + index / 64] & (1 << (index % 64)) != 0
    }

    /// Visit subscribers in installation order, without allocating.
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
    words: &'a [u64],
    next_word: usize,
    bits: u64,
}

impl Iterator for Indices<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        while self.bits == 0 {
            self.bits = *self.words.get(self.next_word)?;
            self.next_word += 1;
        }
        let bit = self.bits.trailing_zeros() as usize;
        self.bits &= self.bits - 1;
        Some((self.next_word - 1) * 64 + bit)
    }
}

#[cfg(test)]
mod tests {
    use super::Subscribers;

    #[test]
    fn builds_sparse_sets_in_mod_order() {
        let set = Subscribers::new(2, 70, [(69, 0), (1, 0), (1, 0)]);
        assert!(set.contains_any(0));
        assert!(!set.contains_any(1));
        assert!(set.contains(0, 69));
        assert!(!set.contains(0, 68));
        assert_eq!(set.indices(0).collect::<Vec<_>>(), [1, 69]);
    }
}
