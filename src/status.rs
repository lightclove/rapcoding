//! Live daemon status, shared between the headless loop and the GUI.

/// What the recognizer is doing right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Listening(&'static str),
    Transcribing(&'static str),
    Error(String),
}

impl Status {
    pub fn label(&self) -> String {
        match self {
            Status::Idle => "Готов".into(),
            Status::Listening(l) => format!("Слушаю ({l})"),
            Status::Transcribing(l) => format!("Распознаю ({l})"),
            Status::Error(e) => format!("Ошибка: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_label() {
        assert_eq!(Status::Idle.label(), "Готов");
    }

    #[test]
    fn listening_label() {
        assert_eq!(Status::Listening("ru").label(), "Слушаю (ru)");
    }

    #[test]
    fn transcribing_label() {
        assert_eq!(Status::Transcribing("en").label(), "Распознаю (en)");
    }

    #[test]
    fn error_label() {
        assert_eq!(Status::Error("boom".into()).label(), "Ошибка: boom");
    }
}
