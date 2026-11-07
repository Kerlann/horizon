use super::super::api;
use super::*;

fn fixture() -> (Manifest, Catalog) {
    let pin = "a".repeat(40);
    let manifest = Manifest::parse(&format!("id='example'\nname='Example'\nversion='1.0.0'\napi='0.2'\ncapabilities=['registries']\n[native]\ngame='26.3'\nhorizon='{pin}'\nmixins='{pin}'\n")).unwrap();
    let catalog = Catalog {
        internals: serde_json::from_value(serde_json::json!([{
            "path":"game::run", "kind":"fn", "signature":"fn run()", "hash":"signature", "cfg":"", "shape_only":false
        }])).unwrap(),
        points: serde_json::from_value(serde_json::json!([{
            "name":"run", "target":"game::run", "raw":true, "at_return":true,
            "listener":"game::__RunListener", "return_listener":"game::__RunReturnListener"
        }])).unwrap(),
        sites: serde_json::from_value(serde_json::json!([{
            "name":"Sone", "target":"game::run", "selector":"self.step", "ordinal":0,
            "hash":"context", "listener":"game::__SoneListener", "record":"game::__SoneArgs", "output":"()"
        }])).unwrap(),
        discovery: false,
    };
    (manifest, catalog)
}

#[test]
fn every_native_kind_and_access_registers_or_refuses_by_name() {
    for kind in [
        Kind::Head,
        Kind::Return,
        Kind::ModifyArg,
        Kind::Replace,
        Kind::BeforeCall,
        Kind::AfterCall,
        Kind::ModifyCallArg,
        Kind::ModifyValue,
        Kind::WrapCall,
        Kind::WrapIf,
        Kind::ModifyLocal,
        Kind::Field,
        Kind::Expression,
    ] {
        for access in [Access::Api, Access::Internals] {
            let (mut manifest, catalog) = fixture();
            let call = matches!(
                kind,
                Kind::BeforeCall
                    | Kind::AfterCall
                    | Kind::ModifyCallArg
                    | Kind::ModifyValue
                    | Kind::WrapCall
                    | Kind::WrapIf
                    | Kind::ModifyLocal
            );
            manifest.inject.push(Inject {
                id: "test".into(),
                at: "game::run".into(),
                kind,
                with: "listen".into(),
                access,
                priority: 7,
                hash: "signature".into(),
                call: call.then(|| "self.step".into()),
                nth: call.then_some(0),
                ctx: call.then(|| "context".into()),
            });
            let pending = match (kind, access) {
                (Kind::ModifyLocal, _) => Some(NoSlotReason::LocalSlotsPending),
                (Kind::Field, _) => Some(NoSlotReason::ExtensionSlotsPending),
                (Kind::Expression, _) => Some(NoSlotReason::SourcePending),
                (Kind::Replace, _) => Some(NoSlotReason::ReplacePending),
                (_, Access::Api) if call => Some(NoSlotReason::ApiSitesPending),
                _ => None,
            };
            if let Some(expected) = pending {
                assert!(
                    matches!(check(&catalog, &manifest), Err(InjectError::NoSlot { reason, .. }) if reason == expected),
                    "{kind:?} {access:?}"
                );
                continue;
            }
            check(&catalog, &manifest).unwrap();
            let facade = catalog.facade(&manifest).unwrap();
            let main = api::main_rs(
                "/* MOD_SLOTS */",
                &[&manifest],
                &catalog.points,
                &catalog.sites,
            );
            if access == Access::Api {
                assert!(
                    main.contains(if kind == Kind::Return {
                        "mod_points::listeners::run_return"
                    } else {
                        "mod_points::listeners::run"
                    }),
                    "{main}"
                );
                assert!(main.contains("(\"example\", 7, \"test\","), "{main}");
                assert!(main.contains("mod_0::listen"), "{main}");
                continue;
            }
            assert_eq!(main.matches("mod_0_internals::raw::").count(), 1, "{main}");
            assert!(main.contains("mod_0::listen"), "{main}");
            assert!(
                main.contains(if call {
                    "(\"example\", 7, \"test\","
                } else {
                    ".ordered(7, \"test\")"
                }),
                "{main}"
            );
            assert!(facade.source.contains(if call {
                "pub const fn sone"
            } else if kind == Kind::Return {
                "pub const fn run_return"
            } else {
                "pub const fn run"
            }));
            let reports = report(&catalog, &manifest).collect::<Vec<_>>();
            assert_eq!(reports.len(), 1);
            assert!(reports[0].contains("example:test: game::run -> slot"));
        }
    }
}

#[test]
fn unified_route_checks_freshness_and_selects_native_function_source() {
    let (mut manifest, mut catalog) = fixture();
    manifest.inject.push(Inject {
        id: "test".into(),
        at: "game::run".into(),
        kind: Kind::WrapIf,
        with: "listen".into(),
        access: Access::Internals,
        priority: 0,
        hash: "signature".into(),
        call: Some("self.step".into()),
        nth: Some(0),
        ctx: Some("context".into()),
    });
    catalog.sites[0].output = "u32".into();
    assert!(matches!(
        check(&catalog, &manifest),
        Err(InjectError::Invalid { field: "kind", .. })
    ));
    manifest.inject[0].hash = "stale".into();
    assert!(matches!(
        check(&catalog, &manifest),
        Err(InjectError::Changed { .. })
    ));
    manifest.inject[0].hash = "signature".into();
    manifest.inject[0].call = None;
    manifest.inject[0].kind = Kind::Head;
    catalog.points.clear();
    check(&catalog, &manifest).unwrap();
    assert!(report(&catalog, &manifest)
        .next()
        .unwrap()
        .contains("source (private slot"));
    manifest.inject[0].kind = Kind::ModifyArg;
    assert!(matches!(
        check(&catalog, &manifest),
        Err(InjectError::NoSlot {
            reason: NoSlotReason::SourcePending,
            ..
        })
    ));
    manifest.inject[0].kind = Kind::Head;
    catalog.internals.clear();
    assert!(matches!(
        check(&catalog, &manifest),
        Err(InjectError::Missing { .. })
    ));
}
