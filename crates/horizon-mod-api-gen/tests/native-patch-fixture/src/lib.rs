#[cfg(test)]
mod model {
    use std::collections::{BTreeMap, BTreeSet};
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
    fn malformed_values_and_reference_retargeting_refuse() {
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
            let mut plan = PatchPlan::default();
            assert!(plan_ref(&mut reference, &trees[0], &mut plan).is_err());
        }
        {
            let mut reference = &number;
            let mut plan = PatchPlan::default();
            assert!(plan_shared(&mut reference, &trees[0], &mut plan).is_err());
        }
        assert_eq!(number, 10);
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
