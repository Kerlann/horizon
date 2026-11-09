// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
use mod_api::{
    RowError, RowWindow,
    wire::{Put, Reader},
};
use mod_points::{
    locators::{Locators, Step},
    native_read::{NativeNode, read},
};

struct Numbers(Vec<u32>);
struct Number(u32);
impl NativeNode for Number {
    fn key(&self) -> u64 {
        2
    }
    fn value(
        &self,
        f: &mut dyn FnMut(Option<&dyn Put>) -> Result<(), RowError>,
    ) -> Result<(), RowError> {
        f(Some(&self.0))
    }
    fn len(&self) -> u64 {
        0
    }
    fn child(
        &self,
        _: u64,
        _: &mut dyn FnMut(Step, u32, &dyn NativeNode) -> Result<(), RowError>,
    ) -> Result<(), RowError> {
        Err(RowError::Bounds)
    }
}
impl NativeNode for Numbers {
    fn key(&self) -> u64 {
        1
    }
    fn value(
        &self,
        f: &mut dyn FnMut(Option<&dyn Put>) -> Result<(), RowError>,
    ) -> Result<(), RowError> {
        f(Some(&(self.0.len() as u64)))
    }
    fn len(&self) -> u64 {
        self.0.len() as u64
    }
    fn child(
        &self,
        i: u64,
        f: &mut dyn FnMut(Step, u32, &dyn NativeNode) -> Result<(), RowError>,
    ) -> Result<(), RowError> {
        let n = self
            .0
            .get(usize::try_from(i).map_err(|_| RowError::Bounds)?)
            .ok_or(RowError::Bounds)?;
        f(
            Step::Index(i),
            u32::try_from(i).map_err(|_| RowError::Bounds)?,
            &Number(*n),
        )
    }
}
#[test]
fn resolves_paths_against_current_value_and_refuses_removed_element() {
    let mut paths = Locators::default();
    let root = paths.intern(None, Step::Root(0)).unwrap();
    let prefix = paths.intern(Some(root), Step::Field(2)).unwrap();
    let unrelated = paths.intern(Some(root), Step::Field(3)).unwrap();
    let req = |node, start, rows, depth| RowWindow {
        node,
        start,
        rows,
        depth,
    };
    assert!(matches!(
        read(
            &Numbers(vec![10]),
            prefix,
            &mut paths,
            req(unrelated, 0, 1, 0)
        ),
        Err(RowError::Bounds)
    ));
    let first = read(
        &Numbers(vec![10, 20]),
        prefix,
        &mut paths,
        req(prefix, 1, 2, 1),
    )
    .unwrap();
    assert_eq!(Reader::decode::<u32>(&first[1].4), Ok(20));
    let id = first[1].0;
    let second = read(&Numbers(vec![30, 40]), prefix, &mut paths, req(id, 0, 1, 0)).unwrap();
    assert_eq!(Reader::decode::<u32>(&second[0].4), Ok(40));
    assert!(matches!(
        read(&Numbers(vec![30]), prefix, &mut paths, req(id, 0, 1, 0)),
        Err(RowError::Bounds)
    ));
}
