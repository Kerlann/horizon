//! Source-owned heavy-read dependency keys. Domains extend this one set; an
//! unrelated light patch must never poison every heavy read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EntityReadDependency {
    Nbt,
}
