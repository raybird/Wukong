use crate::error::{MemoryError, Result};
use std::fmt;

/// Memory isolation boundary. Serialized on the wire as a plain string:
/// `global`, `project:Name`, `agent:Name`, `user:Name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Global,
    Project(String),
    Agent(String),
    User(String),
}

impl Scope {
    /// Parse a scope string. Returns `InvalidScope` for unknown prefixes or
    /// empty names.
    pub fn parse(s: &str) -> Result<Scope> {
        let s = s.trim();
        if s == "global" {
            return Ok(Scope::Global);
        }
        let (prefix, name) = s
            .split_once(':')
            .ok_or_else(|| MemoryError::InvalidScope(s.to_string()))?;
        if name.is_empty() {
            return Err(MemoryError::InvalidScope(s.to_string()));
        }
        match prefix {
            "project" => Ok(Scope::Project(name.to_string())),
            "agent" => Ok(Scope::Agent(name.to_string())),
            "user" => Ok(Scope::User(name.to_string())),
            _ => Err(MemoryError::InvalidScope(s.to_string())),
        }
    }

    /// The scope itself plus its parent scopes, most-specific first.
    /// Every non-global scope falls back to `global`.
    pub fn ancestry(&self) -> Vec<Scope> {
        match self {
            Scope::Global => vec![Scope::Global],
            other => vec![other.clone(), Scope::Global],
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scope::Global => write!(f, "global"),
            Scope::Project(n) => write!(f, "project:{n}"),
            Scope::Agent(n) => write!(f, "agent:{n}"),
            Scope::User(n) => write!(f, "user:{n}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_global() {
        assert_eq!(Scope::parse("global").unwrap(), Scope::Global);
    }

    #[test]
    fn parses_prefixed_scopes() {
        assert_eq!(
            Scope::parse("project:Wukong").unwrap(),
            Scope::Project("Wukong".to_string())
        );
        assert_eq!(
            Scope::parse("agent:main").unwrap(),
            Scope::Agent("main".to_string())
        );
    }

    #[test]
    fn rejects_unknown_prefix_and_empty_name() {
        assert!(Scope::parse("team:x").is_err());
        assert!(Scope::parse("project:").is_err());
        assert!(Scope::parse("nonsense").is_err());
    }

    #[test]
    fn display_roundtrips() {
        let s = Scope::Project("Wukong".to_string());
        assert_eq!(Scope::parse(&s.to_string()).unwrap(), s);
    }

    #[test]
    fn ancestry_appends_global() {
        let a = Scope::Agent("main".to_string()).ancestry();
        assert_eq!(a, vec![Scope::Agent("main".to_string()), Scope::Global]);
        assert_eq!(Scope::Global.ancestry(), vec![Scope::Global]);
    }
}
