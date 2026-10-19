/// Post-processing for ASR transcripts: sound-tag removal, voice commands,
/// and punctuation-friendly spacing. Pure and unit-tested.
pub fn postprocess(
    raw: &str,
    voice_commands: &str,
    remove_sound_tags: bool,
    strip_punct: bool,
) -> String {
    let mut s = raw.replace("\r\n", "\n").replace('\r', "\n");
    if remove_sound_tags {
        s = strip_sound_tags(&s);
    }
    s = collapse_whitespace(&s);
    match voice_commands {
        "off" => {}
        "aggressive" => s = apply_voice_commands(&s, true),
        _ => s = apply_voice_commands(&s, false),
    }
    s = normalize_spacing(&s);
    if strip_punct {
        s = strip_punctuation(&s);
    }
    trim_lines(&s).trim().to_string()
}

/// Remove sentence punctuation and quotes, keeping words (and code symbols
/// like brackets/slashes/equals) intact.
pub fn strip_punctuation(input: &str) -> String {
    let filtered: String = input
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '.' | ','
                    | '!'
                    | '?'
                    | ';'
                    | ':'
                    | '«'
                    | '»'
                    | '„'
                    | '“'
                    | '"'
                    | '\''
                    | '…'
                    | '—'
                    | '–'
            )
        })
        .collect();
    collapse_whitespace(&filtered)
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        match c {
            ' ' | '\t' => {
                if !in_ws {
                    out.push(' ');
                }
                in_ws = true;
            }
            '\n' => {
                while out.ends_with(' ') {
                    out.pop();
                }
                out.push('\n');
                in_ws = true;
            }
            _ => {
                out.push(c);
                in_ws = false;
            }
        }
    }
    out
}

fn trim_lines(s: &str) -> String {
    s.lines().map(|l| l.trim()).collect::<Vec<_>>().join("\n")
}

fn strip_sound_tags(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c == '[' || c == '(' {
            let close = if c == '[' { ']' } else { ')' };
            if let Some(j) = find_closing(&chars, i + 1, close) {
                let inner: String = chars[i + 1..j].iter().collect();
                if is_sound_tag(&inner) {
                    i = j + 1;
                    continue;
                }
            }
            out.push(c);
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn find_closing(chars: &[char], from: usize, close: char) -> Option<usize> {
    for j in from..chars.len() {
        if chars[j] == close {
            return Some(j);
        }
    }
    None
}

fn is_sound_tag(inner: &str) -> bool {
    let t = inner.trim().to_lowercase();
    matches!(
        t.as_str(),
        "music"
            | "applause"
            | "laughter"
            | "silence"
            | "blank_audio"
            | "blank audio"
            | "inaudible"
            | "sound"
            | "noise"
            | "coughing"
            | "sigh"
            | "music_1"
            | "музыка"
            | "смех"
            | "тишина"
            | "шум"
            | "аплодисменты"
            | "вздох"
    )
}

pub fn apply_voice_commands(text: &str, aggressive: bool) -> String {
    let cmds = command_list(aggressive);
    let mut out_lines: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let words: Vec<String> = line.split_whitespace().map(|s| s.to_string()).collect();
        let mut res: Vec<String> = Vec::with_capacity(words.len());
        let mut i = 0;
        while i < words.len() {
            let mut matched = false;
            for (phrase, sym) in &cmds {
                let plen = phrase.len();
                if i + plen > words.len() {
                    continue;
                }
                let mut ok = true;
                for k in 0..plen {
                    if normalize_word(&words[i + k]) != phrase[k] {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    res.push((*sym).to_string());
                    i += plen;
                    matched = true;
                    break;
                }
            }
            if !matched {
                res.push(words[i].clone());
                i += 1;
            }
        }
        out_lines.push(res.join(" "));
    }
    out_lines.join("\n")
}

fn normalize_word(w: &str) -> String {
    w.trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

fn command_list(aggressive: bool) -> Vec<(&'static [&'static str], &'static str)> {
    let mut v: Vec<(&'static [&'static str], &'static str)> = SAFE_COMMANDS.to_vec();
    if aggressive {
        v.extend_from_slice(AGGRESSIVE_COMMANDS);
    }
    v.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    v
}

const SAFE_COMMANDS: &[(&[&str], &str)] = &[
    (&["новая", "строка"], "\n"),
    (&["новый", "абзац"], "\n\n"),
    (&["новый", "параграф"], "\n\n"),
    (&["точка", "с", "запятой"], ";"),
    (&["вопросительный", "знак"], "?"),
    (&["восклицательный", "знак"], "!"),
    (&["открыть", "скобку"], "("),
    (&["закрыть", "скобку"], ")"),
    (&["открыть", "квадратную", "скобку"], "["),
    (&["закрыть", "квадратную", "скобку"], "]"),
    (&["открыть", "фигурную", "скобку"], "{"),
    (&["закрыть", "фигурную", "скобку"], "}"),
    (&["открыть", "кавычки"], "\""),
    (&["закрыть", "кавычки"], "\""),
    (&["двойное", "двоеточие"], "::"),
    (&["обратный", "слэш"], "\\"),
    (&["нижнее", "подчеркивание"], "_"),
    (&["new", "line"], "\n"),
    (&["new", "paragraph"], "\n\n"),
    (&["question", "mark"], "?"),
    (&["exclamation", "mark"], "!"),
    (&["open", "parenthesis"], "("),
    (&["close", "parenthesis"], ")"),
    (&["open", "bracket"], "["),
    (&["close", "bracket"], "]"),
    (&["open", "brace"], "{"),
    (&["close", "brace"], "}"),
    (&["open", "quote"], "\""),
    (&["close", "quote"], "\""),
    (&["double", "colon"], "::"),
    (&["double", "slash"], "//"),
    (&["equal", "sign"], "="),
    (&["at", "sign"], "@"),
    (&["full", "stop"], "."),
    (&["fat", "arrow"], "=>"),
];

const AGGRESSIVE_COMMANDS: &[(&[&str], &str)] = &[
    (&["запятая"], ","),
    (&["точка"], "."),
    (&["двоеточие"], ":"),
    (&["дефис"], "-"),
    (&["тире"], "-"),
    (&["слэш"], "/"),
    (&["равно"], "="),
    (&["пробел"], " "),
    (&["таб"], "\t"),
    (&["стрелка"], "->"),
    (&["бэктик"], "`"),
    (&["comma"], ","),
    (&["period"], "."),
    (&["colon"], ":"),
    (&["semicolon"], ";"),
    (&["hyphen"], "-"),
    (&["dash"], "-"),
    (&["slash"], "/"),
    (&["backslash"], "\\"),
    (&["equals"], "="),
    (&["space"], " "),
    (&["tab"], "\t"),
    (&["plus"], "+"),
    (&["minus"], "-"),
    (&["asterisk"], "*"),
    (&["star"], "*"),
    (&["hash"], "#"),
    (&["ampersand"], "&"),
    (&["pipe"], "|"),
    (&["dot"], "."),
    (&["underscore"], "_"),
    (&["backtick"], "`"),
    (&["arrow"], "->"),
];

/// Remove spaces before punctuation/closing brackets and after opening
/// brackets; collapse the rest. Leaves word spacing otherwise intact.
pub fn normalize_spacing(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c == ' ' || c == '\t' {
            let prev = out.chars().rev().find(|&c| c != ' ');
            let next = next_nonspace(&chars, i + 1);
            if should_keep_space(prev, next) {
                out.push(' ');
            }
            while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                i += 1;
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out.trim().to_string()
}

fn next_nonspace(chars: &[char], mut j: usize) -> Option<char> {
    while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') {
        j += 1;
    }
    if j < chars.len() {
        Some(chars[j])
    } else {
        None
    }
}

fn should_keep_space(prev: Option<char>, next: Option<char>) -> bool {
    let (Some(p), Some(n)) = (prev, next) else {
        return false;
    };
    if n == '\n' || p == '\n' {
        return false;
    }
    if matches!(n, ',' | '.' | ';' | ':' | '!' | '?' | ')' | ']' | '}') {
        return false;
    }
    if matches!(p, '(' | '[' | '{') {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_sound_tags() {
        assert_eq!(strip_sound_tags("[BLANK_AUDIO] hi (music)"), " hi ");
        assert_eq!(strip_sound_tags("[1, 2] ok"), "[1, 2] ok");
    }

    #[test]
    fn safe_voice_commands() {
        // interior newline is preserved
        let out = postprocess("одна новая строка два", "safe", true, false);
        assert_eq!(out, "одна\nдва");
        // single-word "запятая" is NOT replaced in safe mode
        let out = postprocess("слово запятая слово", "safe", true, false);
        assert_eq!(out, "слово запятая слово");
    }

    #[test]
    fn aggressive_voice_commands() {
        let out = postprocess("слово запятая слово", "aggressive", true, false);
        assert_eq!(out, "слово, слово");
    }

    #[test]
    fn english_commands() {
        let out = postprocess(
            "let x equals one new line and two",
            "aggressive",
            true,
            false,
        );
        assert_eq!(out, "let x = one\nand two");
    }

    #[test]
    fn spacing_normalization() {
        assert_eq!(normalize_spacing("слово , другое"), "слово, другое");
        assert_eq!(normalize_spacing("( x )"), "(x)");
        assert_eq!(normalize_spacing("a , b , c"), "a, b, c");
        assert_eq!(normalize_spacing("x \n  y"), "x\ny");
    }

    #[test]
    fn voice_commands_off() {
        let out = postprocess("точка с запятой", "off", true, false);
        assert_eq!(out, "точка с запятой");
    }

    #[test]
    fn strip_punctuation_removes_marks() {
        let out = postprocess("Как ты? Хорошо, спасибо!", "off", false, true);
        assert_eq!(out, "Как ты Хорошо спасибо");
    }
}
