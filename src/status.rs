//! Live daemon status, shared between the headless loop and the GUI.

/// What the recognizer is doing right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Listening(&'static str),
    Transcribing(&'static str),
    Error(String),
}
