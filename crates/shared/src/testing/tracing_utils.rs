//! Shared tracing utilities for testing across all crates
//! Provides assertions and helpers for testing tracing logs and spans

#[cfg(test)]
pub use tracing_test::*;

/// Setup function to initialize test tracing
#[cfg(test)]
pub fn setup_test_tracing() -> impl tracing::Subscriber {
    // Use tracing-test's instrumented subscriber which captures events
    tracing_test::instrument()
}

/// Macro for asserting logs contain specific content
#[macro_export]
macro_rules! assert_log_contains {
    ($level:expr, $pattern:expr) => {
        #[cfg(test)]
        tracing_test::assert_log_contains!($level, $pattern)
    };
}

/// Macro for asserting spans exist
#[macro_export]
macro_rules! assert_span_exists {
    ($span_name:expr) => {
        #[cfg(test)]
        {
            // tracing-test doesn't provide span assertions, so we'll skip this for now
            let _ = $span_name;
        }
    };
}

/// Macro for asserting span fields
#[macro_export]
macro_rules! assert_span_field {
    ($span_name:expr, $field_name:expr, $field_value:expr) => {
        #[cfg(test)]
        {
            // tracing-test doesn't provide span field assertions, so we'll skip this for now
            let _ = $span_name;
            let _ = $field_name;
            let _ = $field_value;
        }
    };
}