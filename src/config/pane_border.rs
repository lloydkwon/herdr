//! Configuration for what a split pane's top border says.
//!
//! The panes on screen all belong to one tab of one workspace, so a border
//! that named the workspace would print the same word on every pane and repeat
//! what the sidebar and the tab bar already say. What actually differs pane to
//! pane is where it is working, what agent is in it, and what that agent is
//! doing — so the border title is a list of tokens the user picks from rather
//! than a fixed string.
//!
//! Token names match the sidebar's (`super::sidebar`) wherever they mean the
//! same thing, so the two surfaces share one vocabulary. Unlike the sidebar,
//! these carry no per-token style: a border title is written in one style whose
//! colour already means something — accent when the pane is focused, muted
//! otherwise — and per-token colours would fight that.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// One field of a pane border title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneBorderToken {
    /// The pane's working directory, abbreviated against `$HOME`.
    Cwd,
    /// The detected or reported agent.
    Agent,
    /// A glyph for the agent's state.
    StateIcon,
    /// The agent's state in words.
    StateText,
    /// The workspace's git branch.
    Branch,
    /// The pane's number within its workspace.
    Pane,
    /// The workspace label. The same on every visible pane.
    Workspace,
    /// The tab label. Also the same on every visible pane.
    Tab,
    /// The pane's terminal title.
    TerminalTitle,
    /// The terminal title with an agent's leading status glyph removed.
    TerminalTitleStripped,
    /// A value the agent reported through `pane report-metadata`.
    Custom(String),
}

impl From<String> for PaneBorderToken {
    fn from(name: String) -> Self {
        Self::Custom(name)
    }
}

const BUILTIN_TOKENS: &[(&str, PaneBorderToken)] = &[
    ("cwd", PaneBorderToken::Cwd),
    ("agent", PaneBorderToken::Agent),
    ("state_icon", PaneBorderToken::StateIcon),
    ("state_text", PaneBorderToken::StateText),
    ("branch", PaneBorderToken::Branch),
    ("pane", PaneBorderToken::Pane),
    ("workspace", PaneBorderToken::Workspace),
    ("tab", PaneBorderToken::Tab),
    ("terminal_title", PaneBorderToken::TerminalTitle),
    (
        "terminal_title_stripped",
        PaneBorderToken::TerminalTitleStripped,
    ),
];

impl PaneBorderToken {
    fn name(&self) -> String {
        match self {
            Self::Cwd => "cwd".into(),
            Self::Agent => "agent".into(),
            Self::StateIcon => "state_icon".into(),
            Self::StateText => "state_text".into(),
            Self::Branch => "branch".into(),
            Self::Pane => "pane".into(),
            Self::Workspace => "workspace".into(),
            Self::Tab => "tab".into(),
            Self::TerminalTitle => "terminal_title".into(),
            Self::TerminalTitleStripped => "terminal_title_stripped".into(),
            Self::Custom(name) => format!("${name}"),
        }
    }
}

impl Serialize for PaneBorderToken {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.name())
    }
}

impl<'de> Deserialize<'de> for PaneBorderToken {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        parse_token(value).map_err(serde::de::Error::custom)
    }
}

fn parse_token(value: String) -> Result<PaneBorderToken, String> {
    if let Some((_, token)) = BUILTIN_TOKENS.iter().find(|(name, _)| *name == value) {
        return Ok(token.clone());
    }
    let Some(name) = value.strip_prefix('$') else {
        return Err(format!(
            "unknown pane border token `{value}`; custom tokens must start with `$`"
        ));
    };
    if name.is_empty()
        || name.len() > 32
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        return Err(format!("invalid custom pane border token `{value}`"));
    }
    Ok(PaneBorderToken::Custom(name.to_string()))
}

/// `[ui.pane_border]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PaneBorderConfig {
    /// Fields to write on the top border, left to right. Empty draws nothing,
    /// which is how the title is turned off.
    pub title: Vec<PaneBorderToken>,
    /// Give an unsplit pane a top border to carry the title. Default: true.
    pub show_when_single_pane: bool,
}

impl Default for PaneBorderConfig {
    /// Where the pane is working and what is running there — the two facts that
    /// actually differ between panes on the same screen. Everything else is
    /// opt-in, because every token costs width the title has to truncate.
    ///
    /// A lone pane is titled too: it is the pane a session spends most of its
    /// time in, and a rule under the tab row costs it one line.
    fn default() -> Self {
        Self {
            title: vec![PaneBorderToken::Cwd, PaneBorderToken::Agent],
            show_when_single_pane: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml: &str) -> Result<PaneBorderConfig, toml::de::Error> {
        toml::from_str(toml)
    }

    #[test]
    fn default_names_where_and_what() {
        assert_eq!(
            PaneBorderConfig::default().title,
            vec![PaneBorderToken::Cwd, PaneBorderToken::Agent]
        );
    }

    // A lone pane is titled by default; the line it costs is opt-out.
    #[test]
    fn single_pane_titles_are_on_by_default_and_can_be_turned_off() {
        assert!(PaneBorderConfig::default().show_when_single_pane);
        let config = parse("show_when_single_pane = false").expect("parses");
        assert!(!config.show_when_single_pane);
        // Turning it off leaves the tokens alone — splits still carry titles.
        assert_eq!(config.title, PaneBorderConfig::default().title);
    }

    #[test]
    fn builtin_tokens_parse() {
        let config = parse(
            r#"title = ["cwd", "agent", "state_icon", "state_text", "branch", "pane",
                       "workspace", "tab", "terminal_title", "terminal_title_stripped"]"#,
        )
        .expect("builtins parse");
        assert_eq!(config.title.len(), 10);
        assert_eq!(config.title[0], PaneBorderToken::Cwd);
        assert_eq!(config.title[9], PaneBorderToken::TerminalTitleStripped);
    }

    #[test]
    fn custom_tokens_use_a_dollar_prefix() {
        let config = parse(r#"title = ["$model"]"#).expect("custom parses");
        assert_eq!(config.title, vec![PaneBorderToken::Custom("model".into())]);
    }

    // A typo should be an error rather than a token that silently never renders.
    #[test]
    fn unknown_tokens_are_rejected() {
        let error = parse(r#"title = ["cdw"]"#).expect_err("typo should fail");
        assert!(
            error.to_string().contains("unknown pane border token"),
            "{error}"
        );
    }

    #[test]
    fn malformed_custom_tokens_are_rejected() {
        for value in ["$", "$has space", "$has/slash"] {
            let toml = format!("title = [\"{value}\"]");
            assert!(parse(&toml).is_err(), "{value} should be rejected");
        }
    }

    // The documented way to turn the title off entirely.
    #[test]
    fn an_empty_list_is_valid() {
        let config = parse("title = []").expect("empty parses");
        assert!(config.title.is_empty());
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(parse(r#"titel = ["cwd"]"#).is_err());
    }

    #[test]
    fn tokens_round_trip_through_serialization() {
        let config = PaneBorderConfig {
            title: vec![
                PaneBorderToken::Cwd,
                PaneBorderToken::Custom("model".into()),
                PaneBorderToken::TerminalTitleStripped,
            ],
            show_when_single_pane: false,
        };
        let encoded = toml::to_string(&config).expect("serializes");
        let decoded: PaneBorderConfig = toml::from_str(&encoded).expect("round trips");
        assert_eq!(decoded, config);
    }
}
