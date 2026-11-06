mod config_files;
pub use config_files::{
    CONFIG_FILE_MAX, CONFIG_LIST_MAX, ConfigEntry, ConfigEntryKind, ConfigFileError, ConfigFiles,
    ConfigPath, ConfigStat,
};
mod __private {
    mod built {
        pub use crate::config_files::built::{ConfigEntry, ConfigStat};
    }
}
