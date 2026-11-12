//! The identity of the code.
//!
//! A symbol of the code is identified by three things: the **module** it
//! belongs to, the **file** it is written in, and the **symbol** itself.
//! Not one and not two.
//!
//! The reason is not pedantry. A name alone is not an identity: two
//! modules can have a function called `parse`, and a file can have two
//! of them. A file alone is not an identity either: a build can produce
//! the same file twice with different contents. And a module alone is not
//! an identity: the same module can be built for two targets.
//!
//! The identity of the code is also the thing that lets a **vulnerable
//! symbol** be matched with what Hodei analysed. The match is on the
//! identity and not on a name, and that is why a false "reachable" is
//! not easy to produce by accident.

use serde::{Deserialize, Serialize};

/// The identity of a symbol of the code.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CodeIdentity {
    /// The module the symbol belongs to.
    pub module: String,
    /// The file the symbol is written in.
    pub file: String,
    /// The symbol itself.
    pub symbol: String,
}

impl CodeIdentity {
    /// The identity of a symbol.
    pub fn new(
        module: impl Into<String>,
        file: impl Into<String>,
        symbol: impl Into<String>,
    ) -> Self {
        Self {
            module: module.into(),
            file: file.into(),
            symbol: symbol.into(),
        }
    }

    /// Whether two identities are the same symbol.
    pub fn is_same_as(&self, other: &CodeIdentity) -> bool {
        self == other
    }
}

impl std::fmt::Display for CodeIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}::{}::{}", self.module, self.file, self.symbol)
    }
}

/// A symbol that a vulnerability is about.
///
/// The advisory names a symbol and the analysis names symbols, and they
/// have to be matched. The match is on the **symbol** and on the
/// **package**, because two packages can have a symbol with the same
/// name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VulnerableSymbol {
    /// The package the symbol belongs to.
    pub package: String,
    /// The version of the package the advisory is about.
    pub version: Option<String>,
    /// The identity of the symbol.
    pub identity: CodeIdentity,
}

impl VulnerableSymbol {
    /// A vulnerable symbol.
    pub fn new(
        package: impl Into<String>,
        version: Option<String>,
        identity: CodeIdentity,
    ) -> Self {
        Self {
            package: package.into(),
            version,
            identity,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_alone_is_not_an_identity() {
        let first = CodeIdentity::new("app", "src/main.rs", "parse");
        let second = CodeIdentity::new("other", "src/main.rs", "parse");
        assert_ne!(
            first, second,
            "two modules can have a function with the same name"
        );
        assert!(!first.is_same_as(&second));
    }

    #[test]
    fn a_file_alone_is_not_an_identity() {
        let first = CodeIdentity::new("app", "src/main.rs", "parse");
        let second = CodeIdentity::new("app", "src/other.rs", "parse");
        assert_ne!(first, second);
    }

    #[test]
    fn the_identity_reads_as_the_three_of_it() {
        let identity = CodeIdentity::new("app", "src/main.rs", "parse");
        assert_eq!(identity.to_string(), "app::src/main.rs::parse");
    }
}
