#![deny(unused_braces)]

#[cfg(test)]
mod model {
    use std::collections::{BTreeMap, BTreeSet};
    pub struct Opaque {
        pub value: u32,
    }
    pub struct Leaf {
        pub value: u32,
    }
    pub struct Game {
        pub hidden: Opaque,
        pub later: Leaf,
        pub other: u32,
    }
    #[derive(Debug, PartialEq)]
    pub struct Data {
        pub list: Vec<u32>,
        pub choice: Choice,
        pub map: BTreeMap<u8, u32>,
        pub set: BTreeSet<u32>,
    }
    #[derive(Debug, PartialEq)]
    pub enum Choice {
        Empty,
        Value(u32),
    }
}
#[cfg(test)]
include!(concat!(env!("OUT_DIR"), "/patch.rs"));
#[cfg(test)]
mod tests {
    use super::*;
    use mod_api::wire::{PatchPlan, RecordPatch};
    use mod_points::{
        locators::{Locators, Step},
        native_patch::PatchTree,
    };
    fn data() -> model::Data {
        model::Data {
            list: vec![10, 20],
            choice: model::Choice::Value(30),
            map: [(1, 40)].into(),
            set: [50].into(),
        }
    }
    fn path(l: &mut Locators, root: u32, steps: &[Step]) -> u32 {
        steps
            .iter()
            .fold(root, |id, s| l.intern(Some(id), *s).unwrap())
    }
    #[test]
    fn opaque_root_refuses_at_runtime() {
        let mut l = Locators::default();
        let root = l.intern(None, Step::Root(1)).unwrap();
        let mut patch = RecordPatch::default();
        patch.set(root, &9u32).unwrap();
        let trees = PatchTree::roots(&l, &[root], &patch).unwrap();
        let mut value = model::Opaque { value: 3 };
        let mut plan = PatchPlan::default();
        assert!(plan_opaque(&mut value, &trees[0], &mut plan).is_err());
        drop(plan);
        assert_eq!(value.value, 3);
    }
    #[test]
    fn partial_policy_is_atomic() {
        let policy = policy();
        let mut l = Locators::default();
        let root = l.intern(None, Step::Root(1)).unwrap();
        let later = path(&mut l, root, &[Step::Field(1), Step::Field(0)]);
        let hidden = path(&mut l, root, &[Step::Field(0)]);
        let other = path(&mut l, root, &[Step::Field(2)]);
        for denied in [None, Some(hidden), Some(other), Some(root)] {
            let mut value = model::Game {
                hidden: model::Opaque { value: 3 },
                later: model::Leaf { value: 4 },
                other: 5,
            };
            let mut p = RecordPatch::default();
            if denied != Some(root) {
                p.set(later, &9u32).unwrap();
            }
            if let Some(id) = denied {
                p.set(id, &7u32).unwrap();
            }
            let trees = PatchTree::roots(&l, &[root], &p).unwrap();
            let mut plan = PatchPlan::default();
            let result = plan_game(&mut value, &trees[0], &mut plan, &policy);
            if denied.is_none() {
                result.unwrap();
                plan.commit().unwrap();
            } else {
                assert!(result.is_err());
                drop(plan);
            }
            assert_eq!(value.later.value, if denied.is_none() { 9 } else { 4 });
            assert_eq!(value.hidden.value, 3);
            assert_eq!(value.other, 5);
        }
    }
    #[test]
    fn tuple_box_option_array_deque_and_whole_replacement() {
        let mut l = Locators::default();
        let root = l.intern(None, Step::Root(81)).unwrap();
        let a = path(
            &mut l,
            root,
            &[
                Step::Field(0),
                Step::Variant(1),
                Step::Field(0),
                Step::Index(1),
            ],
        );
        let b = path(&mut l, root, &[Step::Field(1), Step::Index(0)]);
        let mut value = (
            Box::new(Some([1u32, 2])),
            std::collections::VecDeque::from([3u32]),
        );
        let mut p = RecordPatch::default();
        p.set(a, &9u32).unwrap();
        p.set(b, &8u32).unwrap();
        let trees = PatchTree::roots(&l, &[root], &p).unwrap();
        let mut plan = PatchPlan::default();
        plan_complex(&mut value, &trees[0], &mut plan).unwrap();
        plan.commit().unwrap();
        assert_eq!(value.0.as_ref(), &Some([1, 9]));
        assert_eq!(value.1[0], 8);
        let mut p = RecordPatch::default();
        // Box is transparent on Wire; option and tuple retain their ordinary codecs.
        p.set(root, &(Some([5u32, 6]), vec![7u32, 8])).unwrap();
        let trees = PatchTree::roots(&l, &[root], &p).unwrap();
        let mut plan = PatchPlan::default();
        plan_complex(&mut value, &trees[0], &mut plan).unwrap();
        plan.commit().unwrap();
        assert_eq!(value.0.as_ref(), &Some([5, 6]));
        assert_eq!(value.1.len(), 2);
    }
    #[test]
    fn malformed_values_refuse_and_mutable_references_assign_only_pointees() {
        let mut l = Locators::default();
        let root = l.intern(None, Step::Root(12)).unwrap();
        let leaf = path(&mut l, root, &[Step::Field(0), Step::Index(0)]);
        let choice = path(&mut l, root, &[Step::Field(1)]);
        for trailing in [false, true] {
            let mut p = RecordPatch::default();
            p.set(leaf, &99u32).unwrap();
            if trailing {
                p.set(choice, &(0u32, 9u8)).unwrap();
            } else {
                p.set(choice, &999u32).unwrap();
            }
            let trees = PatchTree::roots(&l, &[root], &p).unwrap();
            let mut a = data();
            let mut plan = PatchPlan::default();
            assert!(plan_data(&mut a, &trees[0], &mut plan).is_err());
            drop(plan);
            assert_eq!(a, data());
        }
        let mut p = RecordPatch::default();
        p.set(root, &99u32).unwrap();
        let trees = PatchTree::roots(&l, &[root], &p).unwrap();
        let mut number = 10u32;
        {
            let mut reference = &mut number;
            let pointer = std::ptr::from_ref(&*reference);
            let mut plan = PatchPlan::default();
            plan_ref(&mut reference, &trees[0], &mut plan).unwrap();
            plan.commit().unwrap();
            assert_eq!(std::ptr::from_ref(&*reference), pointer);
            assert_eq!(*reference, 99);
        }
        {
            let mut reference = &number;
            let mut plan = PatchPlan::default();
            assert!(plan_shared(&mut reference, &trees[0], &mut plan).is_err());
        }
        assert_eq!(number, 99);
        // The shared value codec constructs Data, never an &mut Data. Its ordinary fields
        // replace the pointee at the transparent reference node without retargeting it.
        let mut p = RecordPatch::default();
        p.set(
            root,
            &(vec![1u32], (1u32, 2u32), vec![(3u8, 4u32)], vec![5u32]),
        )
        .unwrap();
        let trees = PatchTree::roots(&l, &[root], &p).unwrap();
        let mut value = data();
        let mut reference = &mut value;
        let pointer = std::ptr::from_ref(&*reference);
        let mut plan = PatchPlan::default();
        plan_ref_data(&mut reference, &trees[0], &mut plan).unwrap();
        plan.commit().unwrap();
        assert_eq!(std::ptr::from_ref(&*reference), pointer);
        assert_eq!(reference.list, vec![1]);
        assert_eq!(reference.choice, model::Choice::Value(2));
        assert_eq!(reference.map[&3], 4);
        assert!(reference.set.contains(&5));
    }
    #[test]
    fn emitted_edits_are_atomic_and_paths_are_current() {
        let mut l = Locators::default();
        let root = l.intern(None, Step::Root(7)).unwrap();
        let other = l.intern(None, Step::Root(9)).unwrap();
        let list = path(&mut l, root, &[Step::Field(0), Step::Index(1)]);
        let variant = path(
            &mut l,
            root,
            &[Step::Field(1), Step::Variant(1), Step::Field(0)],
        );
        let map = path(
            &mut l,
            root,
            &[Step::Field(2), Step::Index(0), Step::Field(1)],
        );
        let key = path(
            &mut l,
            root,
            &[Step::Field(2), Step::Index(0), Step::Field(0)],
        );
        let set = path(&mut l, root, &[Step::Field(3), Step::Index(0)]);
        let bounds = path(&mut l, root, &[Step::Field(0), Step::Index(90000)]);
        for invalid in [key, set, bounds] {
            let mut a = data();
            let before = data();
            let mut p = RecordPatch::default();
            p.set(list, &99u32).unwrap();
            p.set(invalid, &2u32).unwrap();
            let trees = PatchTree::roots(&l, &[root], &p).unwrap();
            let mut plan = PatchPlan::default();
            assert!(plan_data(&mut a, &trees[0], &mut plan).is_err());
            drop(plan);
            assert_eq!(a, before);
        }
        let mut a = data();
        let mut p = RecordPatch::default();
        for id in [list, variant, map] {
            p.set(id, &99u32).unwrap();
        }
        let trees = PatchTree::roots(&l, &[root], &p).unwrap();
        let mut plan = PatchPlan::default();
        plan_data(&mut a, &trees[0], &mut plan).unwrap();
        plan.commit().unwrap();
        assert_eq!(a.list[1], 99);
        assert_eq!(a.choice, model::Choice::Value(99));
        assert_eq!(a.map[&1], 99);
        let mut a = data();
        a.choice = model::Choice::Empty;
        let trees = PatchTree::roots(&l, &[root], &p).unwrap();
        let mut plan = PatchPlan::default();
        assert!(plan_data(&mut a, &trees[0], &mut plan).is_err());
        drop(plan);
        assert_eq!(a.list[1], 20);
        let mut p = RecordPatch::default();
        p.set(list, &99u32).unwrap();
        p.set(other, &999u32).unwrap();
        let trees = PatchTree::roots(&l, &[root, other], &p).unwrap();
        let mut a = data();
        let mut b = data();
        let mut plan = PatchPlan::default();
        plan_data(&mut a, &trees[0], &mut plan).unwrap();
        assert!(plan_data(&mut b, &trees[1], &mut plan).is_err());
        drop(plan);
        assert_eq!(a, data());
        assert_eq!(b, data());
        let trees = PatchTree::roots(&l, &[root, other], &p).unwrap();
        let mut a = data();
        let mut plan = PatchPlan::default();
        assert!(plan_readonly(&mut a, &trees[0], &mut plan).is_err());
    }
}
