use regex::Regex;
use std::error::Error;
use url::{ParseError, Url};
// https://gemini.circumlunar.space/docs/spec-spec.txt

#[derive(Clone, Debug, PartialEq)]
pub enum GeminiType {
    Text,
    Gemini,
}

#[derive(Clone, Debug)]
pub struct GeminiLine {
    // Line type
    pub line_type: LineType,
    pub text: String,
    // TODO: Should be option
    pub url: Option<Url>,
}

impl GeminiLine {
    pub fn parse(line: String, base_url: &Url) -> Result<Self, Box<dyn Error>> {
        let _heading3 = Regex::new(r"^###\s").unwrap();
        let _heading2 = Regex::new(r"^##\s").unwrap();
        let _heading1 = Regex::new(r"^#\s").unwrap();
        let list = Regex::new(r"^\*\s").unwrap();
        let link = Regex::new(r"^=>\s*(.*)$").unwrap();

        // Remove ANSI sequences. Konpeito, I'm looking at you
        let ansi_sequences = Regex::new(r"(\x9B|\x1B\[)[0-?]*[ -/]*[@-~]").unwrap();
        let line = ansi_sequences.replace_all(line.as_str(), "").to_string();

        if line.starts_with("```") {
            return Ok(GeminiLine {
                line_type: LineType::PreformattedToggle,
                text: line,
                url: None,
            });
        }
        if link.is_match(&line) {
            let mut iter = line[2..].trim().split_whitespace();
            let mut url = "";
            if let Some(u) = iter.next() {
                url = u;
            }
            let mut label = iter.collect::<Vec<&str>>().join(" ");
            if label.trim().is_empty() {
                label = url.to_string();
            }
            let mut parsed_url;
            match Url::parse(&url) {
                Ok(u) => parsed_url = u,
                Err(ParseError::RelativeUrlWithoutBase) => {
                    parsed_url = base_url.clone();
                    parsed_url = parsed_url.join(url)?;
                }
                Err(e) => {
                    return Err(Box::new(e));
                }
            }
            let prefix = match parsed_url.scheme() {
                "https" | "http" => "[WWW]".to_string(),
                "gemini" => "[GEM]".to_string(),
                "gopher" => "[GPH]".to_string(),
                "mailto" => "[ \u{2709} ]".to_string(),
                // show first three letters of scheme, lower case to differentiate
                other => format!("[{}]", other.chars().take(3).collect::<String>()),
            };
            return Ok(GeminiLine {
                line_type: LineType::Link,
                text: format!("{}  {}", prefix, label),
                url: Some(parsed_url),
            });
        }
        if list.is_match(&line) {
            return Ok(GeminiLine {
                line_type: LineType::UnorderedList,
                text: line,
                url: None,
            });
        }

        Ok(GeminiLine {
            line_type: LineType::Text,
            text: line,
            url: None,
        })
    }

    pub fn label(self) -> String {
        self.text
    }
}

#[derive(Debug, Eq, PartialEq, Hash, Copy, Clone)]
pub enum LineType {
    Text,
    Link,
    _Preformatted,
    PreformattedToggle,
    _Heading,
    UnorderedList,
}
