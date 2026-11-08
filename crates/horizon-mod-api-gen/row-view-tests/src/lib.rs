//! Opaque layouts have no fabricated scalar or whole-record constructors.
//! ```compile_fail
//! row_view_tests::partial::PartialView::encode_value(&[]);
//! ```
//! ```compile_fail
//! row_view_tests::nominal::RecordView::encode_fields(&[]);
//! ```
//! ```compile_fail
//! fn hidden(value: row_view_tests::nominal::RecordView) {
//!     value.hidden().unwrap().get();
//! }
//! ```
//! ```compile_fail
//! row_view_tests::opaque::OpaqueView::encode_fields(&[]);
//! ```
include!(concat!(env!("OUT_DIR"), "/views.rs"));
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_later_case_keeps_sparse_local_ordinal_and_plan_key() {
        let rows = vec![
            (0, None, 0, partial::KEY, 1u32.to_le_bytes().to_vec()),
            (1, Some(0), 1, 0, vec![]),
            (2, Some(1), 1, 0, 17u16.to_le_bytes().to_vec()),
        ];
        let row = mod_api::RowView::new(rows, None, 0).unwrap();
        assert!(partial::PartialView::new(row.clone(), choice::EXPECTED_ROOT_KEY).is_err());
        let view = partial::PartialView::new(row, partial::KEY).unwrap();
        assert_eq!(view.value_last().unwrap().get(), Ok(17));
    }
    #[test]
    fn emitted_map_accessors_and_constructor() {
        let rows = vec![
            (0, None, 0, 1, 1u64.to_le_bytes().to_vec()),
            (1, Some(0), 0, 0, vec![]),
            (2, Some(1), 0, 0, 8u32.to_le_bytes().to_vec()),
            (3, Some(1), 1, 0, 1u64.to_le_bytes().to_vec()),
            (4, Some(3), 0, 0, 90u16.to_le_bytes().to_vec()),
        ];
        let view = map::MapView::new(mod_api::RowView::new(rows, None, 0).unwrap(), 1).unwrap();
        let (key, value) = view.entry(0).unwrap();
        assert_eq!(key.get(), Ok(8));
        assert_eq!(value.text(1).unwrap(), "Z");
        let bytes =
            map::MapView::encode_entries(&[(&8u32.to_le_bytes(), &[1, 0, 0, 0, 90])]).unwrap();
        let decoded: std::collections::BTreeMap<u32, String> =
            mod_api::wire::Reader::decode(&bytes).unwrap();
        assert_eq!(decoded.get(&8).unwrap(), "Z");
    }
    #[test]
    fn emitted_enum_accessors_check_case() {
        let rows = vec![
            (
                0,
                None,
                0,
                choice::EXPECTED_ROOT_KEY,
                1u32.to_le_bytes().to_vec(),
            ),
            (2, Some(0), 1, 0, vec![]),
            (3, Some(2), 0, 0, 42u32.to_le_bytes().to_vec()),
            (4, Some(2), 1, 0, 17u16.to_le_bytes().to_vec()),
        ];
        let view = choice::ChoiceView::new(
            mod_api::RowView::new(rows, None, 0).unwrap(),
            choice::EXPECTED_ROOT_KEY,
        )
        .unwrap();
        assert_eq!(
            choice::ChoiceView::encode_value(&[&42u32.to_le_bytes(), &17u16.to_le_bytes()])
                .unwrap(),
            vec![1, 0, 0, 0, 42, 0, 0, 0, 17, 0]
        );
        assert!(choice::ChoiceView::encode_value(&[]).is_err());
        assert_eq!(view.tag(), Ok(1));
        assert_eq!(view.value_number().unwrap().get(), Ok(42));
        assert_eq!(view.value_last().unwrap().get(), Ok(17));
        assert!(matches!(view.first_first(), Err(mod_api::RowError::Bounds)));
        let empty = choice::ChoiceView::new(
            mod_api::RowView::new(vec![(0, None, 0, 1, 0u32.to_le_bytes().to_vec())], None, 0)
                .unwrap(),
            1,
        )
        .unwrap();
        assert!(matches!(
            empty.value_number(),
            Err(mod_api::RowError::Bounds)
        ));
    }
    #[test]
    fn opaque_enum_payload_preserves_sibling() {
        let rows = vec![
            (0, None, 0, 90, 0u32.to_le_bytes().to_vec()),
            (1, Some(0), 0, 0, vec![]),
            (5, Some(1), 0, 123, vec![]),
            (8, Some(1), 1, 126, 99u32.to_le_bytes().to_vec()),
        ];
        let view =
            opaque_enum::OpaqueChoiceView::new(mod_api::RowView::new(rows, None, 0).unwrap(), 90)
                .unwrap();
        assert_eq!(view.a_hidden().unwrap().row().type_key(), 123);
        assert_eq!(view.a_visible().unwrap().get(), Ok(99));
    }
    #[test]
    fn opaque_nominal_field_does_not_hide_its_sibling() {
        let rows = vec![
            (0, None, 0, 90, vec![]),
            (5, Some(0), 0, 123, vec![]),
            (8, Some(0), 1, 126, 99u32.to_le_bytes().to_vec()),
        ];
        let view =
            nominal::RecordView::new(mod_api::RowView::new(rows, None, 0).unwrap(), 90).unwrap();
        assert_eq!(view.hidden().unwrap().row().type_key(), 123);
        assert_eq!(view.visible().unwrap().get(), Ok(99));
    }
    #[test]
    fn opaque_descendants_preserve_identity_and_sibling_ordinals() {
        let rows = vec![
            (0, None, 0, 90, vec![]),
            (5, Some(0), 0, 123, vec![]),
            (6, Some(0), 1, 124, 1u64.to_le_bytes().to_vec()),
            (7, Some(6), 0, 125, vec![]),
            (8, Some(0), 2, 126, 99u32.to_le_bytes().to_vec()),
        ];
        let view =
            opaque::OpaqueView::new(mod_api::RowView::new(rows, None, 0).unwrap(), 90).unwrap();
        assert_eq!(view.field_0().unwrap().row().type_key(), 123);
        assert_eq!(view.field_1().unwrap().at(0).unwrap().row().type_key(), 125);
        assert_eq!(view.field_2().unwrap().get(), Ok(99));
    }
    #[test]
    fn emitted_accessors_execute_and_patch_sparse_ids() {
        let rows = vec![
            (0, None, 0, 90, vec![]),
            (5, Some(0), 0, 0, 7u32.to_le_bytes().to_vec()),
            (6, Some(0), 1, 0, 1u32.to_le_bytes().to_vec()),
            (7, Some(6), 1, 0, vec![]),
            (8, Some(7), 0, 0, 1u64.to_le_bytes().to_vec()),
            (9, Some(8), 0, 0, 65u16.to_le_bytes().to_vec()),
            (10, Some(0), 2, 0, 1u64.to_le_bytes().to_vec()),
            (11, Some(10), 0, 0, 12u16.to_le_bytes().to_vec()),
        ];
        let view = Fixture::new(mod_api::RowView::new(rows, None, 0).unwrap(), 90).unwrap();
        assert_eq!(view.field_0().unwrap().get().unwrap(), 7);
        assert_eq!(
            view.field_1()
                .unwrap()
                .value()
                .unwrap()
                .unwrap()
                .text(1)
                .unwrap(),
            "A"
        );
        assert_eq!(view.field_2().unwrap().at(0).unwrap().get().unwrap(), 12);
        assert!(matches!(
            view.field_2().unwrap().at(1),
            Err(mod_api::RowError::Bounds)
        ));
        let mut patch = mod_api::wire::RecordPatch::default();
        view.field_0().unwrap().set(&mut patch, &99).unwrap();
        assert_eq!(
            patch.iter().collect::<Vec<_>>(),
            vec![(5, 99u32.to_le_bytes().as_slice())]
        );
        let bytes =
            Fixture::encode_fields(&[&7u32.to_le_bytes(), &12u16.to_le_bytes(), &[]]).unwrap();
        assert_eq!(bytes, vec![7, 0, 0, 0, 12, 0]);
        assert!(Fixture::encode_fields(&[&vec![0; 1_048_577], &[], &[]]).is_err());
    }
}
