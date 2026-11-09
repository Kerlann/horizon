//! Target selection precedes entry to any native or converted callback scope.
use crate::*;
use std::{cell::RefCell, sync::Arc};
type Recipient = (&'static str, Option<Arc<str>>);
thread_local! {
    static RECIPIENT: RefCell<Option<Recipient>> = const { RefCell::new(None) };
}
struct RecipientScope(Option<Recipient>);
impl Drop for RecipientScope {
    fn drop(&mut self) {
        RECIPIENT.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}
pub(crate) fn with_recipient<R>(
    native: &'static str,
    package: Option<Arc<str>>,
    f: impl FnOnce() -> R,
) -> R {
    let _scope = RecipientScope(RECIPIENT.with(|slot| slot.replace(Some((native, package)))));
    f()
}

pub(crate) fn entered_entity_command_recipient(
    hook_id: generated::HookId,
    hook: &'static str,
) -> impl Iterator<Item = &'static dyn Hooks> {
    let mut recipient = RECIPIENT.with(|slot| slot.borrow().clone());
    // Indexed native identity and subscription bitset: no all-mod scan and no
    // entry to any unrelated listener's native/crash/package scope.
    struct DispatchScope {
        package: Option<crate::crash::ConvertedPackageGuard>,
        native: Option<crate::crash::NativeModGuard>,
    }
    let mut scope = DispatchScope {
        package: None,
        native: None,
    };
    std::iter::once(()).filter_map(move |_| {
        drop(scope.package.take());
        drop(scope.native.take());
        let (native, package) = recipient.take()?;
        let installed = installed()?;
        let &index = installed.by_id.get(native)?;
        if !subscribed_by(index, Some(hook_id)) {
            return None;
        }
        let entry = &installed.mods[index];
        scope.native = Some(mark_mod(entry.id, hook));
        if let Some(package) = package {
            scope.package = Some(crate::crash::enter_entity_command_package(native, package)?);
        }
        Some(&*entry.hooks)
    })
}
