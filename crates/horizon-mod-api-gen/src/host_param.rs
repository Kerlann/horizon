use super::CatalogHook;

/// The hook's host parameter type and the expression passed by its native method.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct HostParam {
    pub ty: String,
    pub call: &'static str,
}

/// Select the host once for both generated signatures and the native call.
pub(super) fn host_param(
    hook: &CatalogHook,
    mutable: Option<bool>,
    ticks: bool,
) -> Option<HostParam> {
    let is_level = hook["target"]
        .as_str()
        .is_some_and(|target| target.contains("::GameLevel<"));
    mutable.map(|mutable| {
        let kind = if is_level {
            match (mutable, ticks) {
                (true, true) => "LevelWriteHost",
                (false, true) => "LevelTicksHost",
                (true, false) => "LevelRandomHost",
                (false, false) => "LevelHost",
            }
        } else {
            "Host"
        };
        let mutable = if mutable || (is_level && ticks) {
            "mut "
        } else {
            ""
        };
        HostParam {
            ty: format!("&{mutable}dyn {kind}"),
            call: if ticks {
                "&mut super::mod_api_host::LevelTicks { level: self, ticks }"
            } else {
                "self"
            },
        }
    })
}
