pub mod model {
    use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
    #[derive(Debug, PartialEq, Eq)]
    pub struct Record {
        pub choice: Choice,
        pub maybe: Option<Box<u32>>,
        pub list: Vec<u8>,
        pub array: [u16; 2],
        pub deque: VecDeque<u8>,
        pub set: BTreeSet<u8>,
        pub hash_set: HashSet<u8>,
        pub map: BTreeMap<u8, u16>,
        pub hash_map: HashMap<u8, u16>,
    }
    #[derive(Debug, PartialEq, Eq)]
    pub enum Choice {
        Empty,
        Pair(u32, bool),
        Named { flag: bool },
    }
}
#[cfg(test)]
include!(concat!(env!("OUT_DIR"), "/codec.rs"));

#[cfg(test)]
mod tests {
    use super::*;
    use mod_api::wire::{DecodedPatchCell, PatchCell, PatchFields, Record, WireError, Writer};
    fn sample() -> model::Record {
        model::Record {
            choice: model::Choice::Pair(17, true),
            maybe: Some(Box::new(3)),
            list: vec![5; 70_000],
            array: [7, 8],
            deque: [1, 2].into(),
            set: [3, 4].into(),
            hash_set: [5, 6].into(),
            map: [(1, 20), (2, 30)].into(),
            hash_map: [(7, 40)].into(),
        }
    }
    fn encoded(value: &model::Record) -> Vec<u8> {
        let mut out = Writer::new();
        encode_record(value, &mut out);
        out.into_bytes()
    }
    fn apply(payload: &[u8], first: &mut u32, value: &mut model::Record) -> Result<(), WireError> {
        // The patch stores already encoded values; its public setter expects a Wire value.
        let mut raw = Writer::new();
        raw.put(&vec![
            (1u32, 42u32.to_le_bytes().to_vec()),
            (2u32, payload.to_vec()),
        ]);
        PatchFields::new(
            (
                PatchCell::new(1, first),
                (DecodedPatchCell::new(2, value, decode_record), ()),
            ),
            3,
        )
        .apply(raw.bytes())
    }
    #[test]
    fn generated_native_round_trip_and_atomic_invalid_cells() {
        let expected = sample();
        let bytes = encoded(&expected);
        let mut first = 9;
        let mut value = sample();
        value.choice = model::Choice::Empty;
        for bad in [
            {
                let mut b = bytes.clone();
                b[0] = 9;
                b
            },
            {
                let mut b = bytes.clone();
                b[8] = 2;
                b
            },
            {
                let mut b = bytes.clone();
                b.push(0);
                b
            },
            bytes[..10].to_vec(),
        ] {
            assert!(apply(&bad, &mut first, &mut value).is_err());
            assert_eq!(first, 9);
            let mut untouched = sample();
            untouched.choice = model::Choice::Empty;
            assert_eq!(value, untouched);
        }
        apply(&bytes, &mut first, &mut value).unwrap();
        assert_eq!(first, 42);
        assert_eq!(value, expected);
        value.choice = model::Choice::Named { flag: false };
        let bytes = encoded(&value);
        let mut reader = mod_api::wire::Reader::new(&bytes);
        assert_eq!(decode_record(&mut reader).unwrap(), value);
    }
}
