pub mod model {
    #[derive(Debug, PartialEq, Eq)]
    pub struct Record {
        pub choice: Choice,
        pub values: Vec<u16>,
    }
    #[derive(Debug, PartialEq, Eq)]
    pub enum Choice {
        Empty,
        Number(u32),
        Named { flag: bool },
    }
}
#[cfg(test)]
include!(concat!(env!("OUT_DIR"), "/callable.rs"));

#[cfg(test)]
mod tests {
    use super::*;
    use mod_api::wire::{Id, Reader, ResourceCodec, ResourceScope, WireError, Writer};
    use std::{
        any::{Any, TypeId},
        cell::{Cell, RefCell},
        rc::Rc,
    };

    struct NoResources;
    impl ResourceCodec for NoResources {
        fn store(&self, _: Box<dyn Any>) -> Result<Id, WireError> {
            Err(WireError::Invalid("unused"))
        }
        fn load(&self, _: Id, _: TypeId) -> Result<Box<dyn Any>, WireError> {
            Err(WireError::Invalid("unused"))
        }
    }
    #[test]
    fn generated_callable_codecs_invoke_fn_mut_with_native_struct_enum_and_collections() {
        let count = Cell::new(0);
        let callback = RefCell::new(
            |(record, choice, extra): (model::Record, model::Choice, Vec<u16>)| {
                count.set(count.get() + 1);
                assert_eq!(
                    record,
                    model::Record {
                        choice: model::Choice::Number(17),
                        values: vec![2, 3]
                    }
                );
                assert_eq!(choice, model::Choice::Named { flag: true });
                assert_eq!(extra, vec![8, 9]);
                model::Record {
                    choice: model::Choice::Number(99),
                    values: vec![10, 11, 12],
                }
            },
        );
        let _scope = unsafe { ResourceScope::native_root(Rc::new(NoResources)) };
        mod_points::snapshot::with_root(&callback, |root| {
            let source = source(&root).unwrap();
            let mut input = Writer::new();
            input.put(&1u32);
            input.put(&17u32);
            input.put(&vec![2u16, 3]);
            input.put(&2u32);
            input.put(&true);
            input.put(&vec![8u16, 9]);
            let mut trailing = input.bytes().to_vec();
            trailing.push(0);
            assert!(source.invoke(trailing).is_err());
            let mut bad = input.bytes().to_vec();
            bad[..4].copy_from_slice(&99u32.to_le_bytes());
            assert!(source.invoke(bad).is_err());
            assert_eq!(count.get(), 0);
            let bytes = source.invoke(input.into_bytes()).unwrap();
            let mut result = Reader::new(&bytes);
            assert_eq!(result.take::<u32>(), Ok(1));
            assert_eq!(result.take::<u32>(), Ok(99));
            assert_eq!(result.take::<Vec<u16>>(), Ok(vec![10, 11, 12]));
            assert!(result.rest().is_empty());
            assert_eq!(count.get(), 1);
        });
    }
}
