#[cfg(test)]
mod tests {
    use mod_api::wire::{RecordPatch, Writer};
    use mod_points::snapshot::Snapshot;

    #[derive(Debug, PartialEq)]
    struct Thing {
        number: u32,
        nested: (bool, [i16; 2]),
        readonly: &'static u8,
    }
    include!(concat!(env!("OUT_DIR"), "/projection.rs"));

    fn bytes(patch: &RecordPatch) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.put(patch);
        writer.into_bytes()
    }

    #[test]
    fn generated_snapshot_and_atomic_patches() {
        let mut value = Thing {
            number: 7,
            nested: (true, [-3, 8]),
            readonly: &42,
        };
        let rows = snapshot(&value).unwrap();
        let view = Snapshot::new(&rows);
        assert_eq!(view.field(0, 4), Some(1));
        assert_eq!(view.field(1, 0), Some(2));
        assert_eq!(view.field(1, 1), Some(3));
        assert_eq!(view.field(3, 0), Some(4));
        assert_eq!(view.field(3, 1), Some(5));
        assert_eq!(view.field(5, 0), Some(6));
        assert_eq!(view.field(5, 1), Some(7));
        assert_eq!(view.field(1, 2), Some(8));
        assert_eq!(view.read::<u32>(2).unwrap(), 7);
        assert_eq!(view.read::<bool>(4).unwrap(), true);
        assert_eq!(view.read::<u64>(5).unwrap(), 2);
        assert_eq!(view.read::<i16>(6).unwrap(), -3);
        assert_eq!(view.read::<i16>(7).unwrap(), 8);
        assert_eq!(view.read::<u8>(8).unwrap(), 42);
        let mut invalid = RecordPatch::default();
        invalid.set(2, &99u32).unwrap();
        invalid.set(7, &true).unwrap();
        assert!(apply(&mut value, &bytes(&invalid)).is_err());
        assert_eq!(value.number, 7);
        assert_eq!(value.nested, (true, [-3, 8]));
        let mut readonly = RecordPatch::default();
        readonly.set(2, &99u32).unwrap();
        readonly.set(8, &1u8).unwrap();
        assert!(apply(&mut value, &bytes(&readonly)).is_err());
        assert_eq!(value.number, 7);
        let mut valid = RecordPatch::default();
        valid.set(2, &99u32).unwrap();
        valid.set(4, &false).unwrap();
        valid.set(6, &12i16).unwrap();
        valid.set(7, &13i16).unwrap();
        apply(&mut value, &bytes(&valid)).unwrap();
        assert_eq!(
            value,
            Thing {
                number: 99,
                nested: (false, [12, 13]),
                readonly: &42
            }
        );
    }
}
