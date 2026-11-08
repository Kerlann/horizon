/// Read-only block metadata and typed operation errors.
pub mod block_metadata;
pub use block_metadata::BlockStateError;
// hidden: low-level resource and derived transport used by BlockState facade operations.
#[doc(hidden)]
pub use block_metadata::{BlockMetadata, BlockStateRecord};
