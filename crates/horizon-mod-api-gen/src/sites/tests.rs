use super::*;

#[test]
fn site_lists_are_a_sorted_union_with_locations_and_checked_syntax() {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("site-lists-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let a = directory.join("a.txt");
    let b = directory.join("b.txt");
    fs::write(
        &a,
        "# comment\ngame::Game<'_>::tick | self.step | 0 # note\ngame::tick | .step | \n",
    )
    .unwrap();
    fs::write(&b, "game::Game::tick | self.step | 0\n").unwrap();
    let declarations = listed(&directory).unwrap();
    assert_eq!(declarations.len(), 2);
    assert_eq!(declarations[0].target, "game::Game::tick");
    assert!(declarations[0].origin.ends_with("a.txt:2"));
    assert_eq!(declarations[1].ordinal, None);
    for (bad, reason) in [
        ("game::tick | self.step", "expected target"),
        ("? | self.step | 0", "invalid syntax"),
        ("game::tick | . | 0", "selector"),
        ("game::tick | .step | -1", "ordinal"),
    ] {
        fs::write(&a, bad).unwrap();
        let error = listed(&directory).unwrap_err();
        assert!(matches!(error.cause, Cause::Syntax(_) | Cause::Format(_)));
        assert!(error.origin.as_ref().unwrap().ends_with("a.txt:1"));
        assert!(error.to_string().contains(reason), "{error}");
    }
    fs::remove_dir_all(&directory).unwrap();
    assert!(listed(&directory).unwrap().is_empty());
}
