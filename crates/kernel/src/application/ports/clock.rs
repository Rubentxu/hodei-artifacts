//! The seams that make a result reproducible.
//!
//! A moment in time and an identifier are not inputs, they are facts about
//! the machine. When the code reads the clock directly, the result depends on
//! when it ran, and two runs of the same operation cannot be compared. That
//! matters where the result is evidence: an audit event whose moment and
//! identifier cannot be reproduced cannot be checked, and a policy whose
//! creation mark cannot be reproduced cannot be tested.
//!
//! The clock and the identifier generator are therefore ports. The
//! implementation that reads the machine is one of them; the one a test uses
//! to pin the result is another.

use chrono::{DateTime, Utc};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// A source of the current time.
pub trait Clock: Send + Sync {
    /// The current moment, in UTC.
    fn now(&self) -> DateTime<Utc>;
}

/// The clock of the machine.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// A clock that stays where it is told to.
///
/// A test that pins the moment gets the same answer every time, which is
/// what makes a comparison between two executions meaningful.
#[derive(Debug, Clone)]
pub struct FixedClock {
    moment: DateTime<Utc>,
}

impl FixedClock {
    /// A clock that always reports this moment.
    pub fn new(moment: DateTime<Utc>) -> Self {
        Self { moment }
    }

    /// A clock that always reports this moment, given as a string.
    pub fn at(moment: &str) -> Result<Self, chrono::ParseError> {
        let parsed = DateTime::parse_from_rfc3339(moment)?;
        Ok(Self::new(parsed.with_timezone(&Utc)))
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.moment
    }
}

/// A source of unique identifiers.
pub trait IdGenerator: Send + Sync {
    /// A new identifier.
    fn new_id(&self) -> String;
}

/// The identifier generator of the machine.
#[derive(Debug, Clone, Copy, Default)]
pub struct UuidGenerator;

impl IdGenerator for UuidGenerator {
    fn new_id(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

/// An identifier generator that counts.
///
/// The identifiers it produces are predictable, which is what a test needs
/// when it wants to assert on an event's identity.
#[derive(Debug, Default)]
pub struct CountingIdGenerator {
    counter: AtomicU64,
    prefix: Mutex<String>,
}

impl CountingIdGenerator {
    /// A generator whose identifiers start with this prefix.
    pub fn with_prefix(prefix: impl Into<String>) -> Self {
        Self {
            counter: AtomicU64::new(0),
            prefix: Mutex::new(prefix.into()),
        }
    }
}

impl IdGenerator for CountingIdGenerator {
    fn new_id(&self) -> String {
        let count = self.counter.fetch_add(1, Ordering::SeqCst);
        let prefix = self
            .prefix
            .lock()
            .map(|prefix| prefix.clone())
            .unwrap_or_default();
        format!("{prefix}{count}")
    }
}

/// The clock and the identifier generator a component works with.
#[derive(Clone)]
pub struct Identity {
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGenerator>,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity").finish_non_exhaustive()
    }
}

impl Identity {
    /// The identity of the machine.
    pub fn system() -> Self {
        Self {
            clock: Arc::new(SystemClock),
            ids: Arc::new(UuidGenerator),
        }
    }

    /// An identity with the given clock and identifier generator.
    pub fn new(clock: Arc<dyn Clock>, ids: Arc<dyn IdGenerator>) -> Self {
        Self { clock, ids }
    }

    /// The current moment.
    pub fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    /// A new identifier.
    pub fn new_id(&self) -> String {
        self.ids.new_id()
    }
}

impl Default for Identity {
    fn default() -> Self {
        Self::system()
    }
}
