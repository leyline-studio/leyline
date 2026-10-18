//! Which program wrote a revision (ADR 0154 §6).
//!
//! The origin is declared once, by the client, when it opens a library, and
//! the engine stamps it on every revision that handle writes. It is a label,
//! not an identity: an MCP client names itself, and nothing checks the name.

use std::fmt;

/// The longest client name an MCP origin keeps, in characters.
pub const MCP_CLIENT_NAME_MAX: usize = 64;

/// The program that wrote a develop revision.
///
/// Stored in `develop_revisions.origin` as `studio`, `cli` or `mcp:<client>`.
/// A revision written before catalog v15, or by a handle that declared no
/// origin, has none: that is read as *unknown*, never guessed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RevisionOrigin {
    /// Leyline Studio.
    Studio,
    /// The `leyline` command line.
    Cli,
    /// An agent over MCP, with the client name it declared at `initialize`.
    Mcp(String),
}

impl RevisionOrigin {
    /// An MCP origin from the self-declared client name: trimmed, stripped
    /// of control characters and capped at [`MCP_CLIENT_NAME_MAX`]
    /// characters. A name that is empty once cleaned becomes `unnamed`, so
    /// the stored value always says it came from an agent.
    pub fn mcp(client_name: &str) -> RevisionOrigin {
        let cleaned: String = client_name
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(MCP_CLIENT_NAME_MAX)
            .collect();
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            RevisionOrigin::Mcp("unnamed".to_owned())
        } else {
            RevisionOrigin::Mcp(cleaned.to_owned())
        }
    }

    /// Reads the stored form. A value this engine does not recognise — one a
    /// newer engine wrote — is `None`, read as unknown like a missing one.
    pub fn parse(stored: &str) -> Option<RevisionOrigin> {
        match stored {
            "studio" => Some(RevisionOrigin::Studio),
            "cli" => Some(RevisionOrigin::Cli),
            other => other
                .strip_prefix("mcp:")
                .map(|client| RevisionOrigin::Mcp(client.to_owned())),
        }
    }
}

impl fmt::Display for RevisionOrigin {
    /// The stored form: `studio`, `cli` or `mcp:<client>`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RevisionOrigin::Studio => f.write_str("studio"),
            RevisionOrigin::Cli => f.write_str("cli"),
            RevisionOrigin::Mcp(client) => write!(f, "mcp:{client}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_form_round_trips() {
        for origin in [
            RevisionOrigin::Studio,
            RevisionOrigin::Cli,
            RevisionOrigin::mcp("Claude Desktop"),
        ] {
            assert_eq!(RevisionOrigin::parse(&origin.to_string()), Some(origin));
        }
    }

    #[test]
    fn client_name_is_cleaned_and_capped() {
        assert_eq!(
            RevisionOrigin::mcp("  agent\u{7}\nname  "),
            RevisionOrigin::Mcp("agentname".to_owned())
        );
        let long = "é".repeat(100);
        let RevisionOrigin::Mcp(name) = RevisionOrigin::mcp(&long) else {
            panic!("mcp() builds an Mcp origin");
        };
        assert_eq!(name.chars().count(), MCP_CLIENT_NAME_MAX);
        assert_eq!(
            RevisionOrigin::mcp(" \u{1b} "),
            RevisionOrigin::Mcp("unnamed".to_owned())
        );
    }

    #[test]
    fn unknown_stored_value_is_unknown() {
        assert_eq!(RevisionOrigin::parse("plugin"), None);
    }
}
