#[cfg(all(feature = "wasm-host", not(target_arch = "wasm32")))]
mod wasm_host;
mod __private {
    mod wasm_host {
        pub use crate::wasm_host::{
            Budgets, CallHost, CallRefused, EPOCH, Exports, Fuel, Instantiated, LoadError, Member,
            State, instantiate, live_groups,
        };
    }
}
