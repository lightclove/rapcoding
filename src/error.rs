#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("config: {0}")]
    Config(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("toml parse: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("toml serialize: {0}")]
    TomlSer(#[from] toml::ser::Error),

    #[error("audio: {0}")]
    Audio(String),

    #[error("asr: {0}")]
    Asr(String),

    #[error("hotkey: {0}")]
    Hotkey(String),

    #[error("inject: {0}")]
    Inject(String),

    #[cfg(feature = "gui")]
    #[error("gui: {0}")]
    Gui(String),

    #[cfg(feature = "gui")]
    #[error("download: {0}")]
    Download(String),
}

pub type Result<T> = std::result::Result<T, Error>;
