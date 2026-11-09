// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
use mod_api::wire::{PatchPlan, RecordPatch, WireError};
use mod_points::{
    locators::{Locators, Step},
    native_patch::PatchTree,
};

#[test]
fn tree_is_source_local_and_rejects_overlaps() {
    let mut locators = Locators::default();
    let root = locators.intern(None, Step::Root(7)).unwrap();
    let prefix = locators.intern(Some(root), Step::Field(4)).unwrap();
    let child = locators.intern(Some(prefix), Step::Index(90_000)).unwrap();
    let other = locators.intern(None, Step::Root(9)).unwrap();
    let mut patch = RecordPatch::default();
    patch.set(child, &42u32).unwrap();
    let roots = PatchTree::roots(&locators, &[prefix, other], &patch).unwrap();
    let (step, subtree) = roots[0].children().next().unwrap();
    assert_eq!(step, Step::Index(90_000));
    assert_eq!(subtree.value(), Some(&42u32.to_le_bytes()[..]));
    assert!(roots[1].is_empty());
    assert!(PatchTree::roots(&locators, &[other], &patch).is_err());
    assert!(PatchTree::roots(&locators, &[root, prefix], &patch).is_err());
    patch.set(prefix, &1u32).unwrap();
    assert!(PatchTree::roots(&locators, &[prefix], &patch).is_err());
    let mut unknown = RecordPatch::default();
    unknown.set(u32::MAX, &1u32).unwrap();
    assert!(PatchTree::roots(&locators, &[root], &unknown).is_err());
}

#[test]
fn typed_candidates_across_roots_are_atomic_on_bad_data() {
    let (mut first, mut second) = (10u32, 20u32);
    {
        let mut plan = PatchPlan::default();
        plan.prepare(&mut first, &5u32.to_le_bytes(), |r| r.take())
            .unwrap();
        assert_eq!(
            plan.prepare(&mut second, &[1, 0, 0, 0, 9], |r| r.take::<u32>()),
            Err(WireError::Trailing)
        );
        assert_eq!(plan.commit(), Err(WireError::Trailing));
    }
    assert_eq!((first, second), (10, 20));
    {
        let mut plan = PatchPlan::default();
        plan.prepare(&mut first, &5u32.to_le_bytes(), |r| r.take())
            .unwrap();
        plan.prepare(&mut second, &6u32.to_le_bytes(), |r| r.take())
            .unwrap();
        plan.commit().unwrap();
    }
    assert_eq!((first, second), (5, 6));
}
