//! The version of a schema, and how an old event is read.
//!
//! Events are immutable. The schema that describes their payload can still
//! change, and the plan says how:
//!
//! - **additive compatible fields**: a new field appears and the old reader
//!   ignores it, because the reader has a default for it;
//! - **reader defaults**: the reader supplies the value the old event did
//!   not have;
//! - **an explicit upcaster**: from version N to version N+1, written on
//!   purpose and applied on purpose;
//! - **a new event type**: when the meaning changes, the old event keeps its
//!   meaning and the new one has a new name.
//!
//! What is forbidden, and the reason this module exists:
//!
//! - reinterpreting an old payload in place -- the payload of an event that
//!   was accepted means what it meant when it was accepted;
//! - silently changing the raw evidence that was stored;
//! - using the debug rendering of a type as a persistence contract.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The version of a schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SchemaVersion {
    /// The incompatible part. A change here means a change of meaning.
    pub major: u16,
    /// The compatible part. A change here means more information.
    pub minor: u16,
}

impl SchemaVersion {
    /// A version.
    pub fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    /// The first version of a schema.
    pub fn initial() -> Self {
        Self { major: 1, minor: 0 }
    }

    /// Whether a reader of this version can read an event of the given
    /// version.
    ///
    /// The same major version is compatible: the reader knows the meaning of
    /// the payload and supplies the defaults of the fields that did not
    /// exist yet. A different major version is not: the payload means
    /// something else, and it needs an upcaster or a new event type.
    pub fn can_read(&self, other: SchemaVersion) -> bool {
        self.major == other.major
    }

    /// The next version, when only more information was added.
    pub fn next_minor(self) -> Self {
        Self {
            major: self.major,
            minor: self.minor + 1,
        }
    }

    /// The next version, when the meaning changed.
    pub fn next_major(self) -> Self {
        Self {
            major: self.major + 1,
            minor: 0,
        }
    }
}

impl fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// The error of an event that a reader cannot read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VersionError {
    /// The event is of a version the reader does not know, and no upcaster
    /// was given for it.
    #[error("the event is of schema version {found} and no upcaster knows how to read it")]
    NoUpcaster {
        /// The version of the event.
        found: SchemaVersion,
    },
    /// The upcaster refused the event.
    #[error("the upcaster refused the event: {0}")]
    UpcasterRefused(String),
}

/// A reader that brings an old event up to the version that is read today.
///
/// The upcaster is explicit: it is a function, written on purpose, and it
/// is the only thing allowed to change the payload of an event that was
/// already written. It never changes the meaning of the event, only its
/// shape, and it says so by refusing anything it cannot handle.
pub trait Upcaster: Send + Sync {
    /// The version this upcaster reads.
    #[allow(clippy::wrong_self_convention)]
    fn from_version(&self) -> SchemaVersion;

    /// The version this upcaster produces.
    fn to_version(&self) -> SchemaVersion;

    /// Brings an event up to the version, or refuses it.
    fn upcast(&self, event: &crate::domain::event::EventEnvelope) -> Result<(), VersionError>;
}

/// The registry of the upcasters that are known.
///
/// The registry answers one question: can this reader read this event? When
/// it cannot, it says which version it found and that no upcaster knows how
/// to read it, which is a better answer than reading it wrongly.
#[derive(Default)]
pub struct Upcasters {
    upcasters: Vec<Box<dyn Upcaster>>,
}

impl Upcasters {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an upcaster.
    pub fn register(&mut self, upcaster: Box<dyn Upcaster>) {
        self.upcasters.push(upcaster);
    }

    /// Whether the registry can bring an event of the given version up to
    /// the version that is read.
    pub fn can_read(&self, from: SchemaVersion, to: SchemaVersion) -> bool {
        if from.can_read(to) {
            return true;
        }
        self.upcasters
            .iter()
            .any(|upcaster| upcaster.from_version() == from && upcaster.to_version() == to)
    }

    /// The upcaster that goes from one version to another.
    pub fn upcaster(&self, from: SchemaVersion, to: SchemaVersion) -> Option<&dyn Upcaster> {
        self.upcasters
            .iter()
            .find(|upcaster| upcaster.from_version() == from && upcaster.to_version() == to)
            .map(|upcaster| upcaster.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_major_version_is_readable_and_another_one_is_not() {
        let reader = SchemaVersion::new(1, 4);
        assert!(reader.can_read(SchemaVersion::new(1, 0)));
        assert!(!reader.can_read(SchemaVersion::new(2, 0)));
    }

    #[test]
    fn the_versions_follow_each_other() {
        assert_eq!(
            SchemaVersion::initial().next_minor(),
            SchemaVersion::new(1, 1)
        );
        assert_eq!(
            SchemaVersion::new(1, 7).next_major(),
            SchemaVersion::new(2, 0)
        );
    }

    #[test]
    fn an_empty_registry_cannot_read_another_major_version() {
        let registry = Upcasters::new();
        assert!(registry.can_read(SchemaVersion::new(1, 0), SchemaVersion::new(1, 0)));
        assert!(!registry.can_read(SchemaVersion::new(1, 0), SchemaVersion::new(2, 0)));
    }
}
