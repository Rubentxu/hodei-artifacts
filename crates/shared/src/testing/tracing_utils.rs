//! Shared tracing utilities for testing across all crates
//! Provides assertions and helpers for testing tracing logs and spans

use std::sync::{Arc, Mutex};
use tracing::{Level, Subscriber};
use tracing_subscriber::{
    layer::{Context, Layer},
    registry::LookupSpan,
    prelude::*,
};

/// A test subscriber that captures all logs and spans for assertions
#[derive(Debug, Default, Clone)]
pub struct TestTracingSubscriber {
    logs: Arc<Mutex<Vec<TestLogEntry>>>,
    spans: Arc<Mutex<Vec<TestSpanEntry>>>,
}

#[derive(Debug, Clone)]
pub struct TestLogEntry {
    pub level: Level,
    pub message: String,
    pub target: String,
}

#[derive(Debug, Clone)]
pub struct TestSpanEntry {
    pub name: String,
    pub fields: Vec<(String, String)>,
}

impl TestTracingSubscriber {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn logs(&self) -> Vec<TestLogEntry> {
        self.logs.lock().unwrap().clone()
    }

    pub fn spans(&self) -> Vec<TestSpanEntry> {
        self.spans.lock().unwrap().clone()
    }

    pub fn clear(&self) {
        self.logs.lock().unwrap().clear();
        self.spans.lock().unwrap().clear();
    }

    /// Assert that a log message with specific content exists
    pub fn assert_log_contains(&self, level: Level, message_pattern: &str) {
        let logs = self.logs();
        assert!(
            logs.iter().any(|log| {
                log.level == level && log.message.contains(message_pattern)
            }),
            "Expected log with level {:?} containing '{}', but found logs: {:?}",
            level,
            message_pattern,
            logs
        );
    }

    /// Assert that a span with specific name exists
    pub fn assert_span_exists(&self, span_name: &str) {
        let spans = self.spans();
        assert!(
            spans.iter().any(|span| span.name == span_name),
            "Expected span '{}', but found spans: {:?}",
            span_name,
            spans
        );
    }

    /// Assert that a span contains specific field values
    pub fn assert_span_contains_field(&self, span_name: &str, field_name: &str, field_value: &str) {
        let spans = self.spans();
        let span = spans
            .iter()
            .find(|span| span.name == span_name)
            .unwrap_or_else(|| {
                panic!(
                    "Span '{}' not found. Available spans: {:?}",
                    span_name, spans
                )
            });

        assert!(
            span.fields
                .iter()
                .any(|(name, value)| name == field_name && value.contains(field_value)),
            "Span '{}' does not contain field '{}' with value '{}'. Fields: {:?}",
            span_name,
            field_name,
            field_value,
            span.fields
        );
    }

    /// Count logs by level
    pub fn count_logs_by_level(&self, level: Level) -> usize {
        self.logs()
            .iter()
            .filter(|log| log.level == level)
            .count()
    }

    /// Count spans by name
    pub fn count_spans_by_name(&self, span_name: &str) -> usize {
        self.spans()
            .iter()
            .filter(|span| span.name == span_name)
            .count()
    }
}

impl<S> Layer<S> for TestTracingSubscriber
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: Context<'_, S>,
    ) {
        let mut visitor = LogVisitor::default();
        event.record(&mut visitor);

        let log_entry = TestLogEntry {
            level: *event.metadata().level(),
            message: visitor.message,
            target: event.metadata().target().to_string(),
        };

        self.logs.lock().unwrap().push(log_entry);
    }

    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        let mut visitor = SpanVisitor::default();
        attrs.record(&mut visitor);

        let span = ctx.span(id).expect("Span not found");
        let metadata = span.metadata();

        let span_entry = TestSpanEntry {
            name: metadata.name().to_string(),
            fields: visitor.fields,
        };

        self.spans.lock().unwrap().push(span_entry);
    }
}

#[derive(Default)]
struct LogVisitor {
    message: String,
}

impl tracing::field::Visit for LogVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        }
    }
}

#[derive(Default)]
struct SpanVisitor {
    fields: Vec<(String, String)>,
}

impl tracing::field::Visit for SpanVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.fields.push((field.name().to_string(), format!("{:?}", value)));
    }
}

/// Setup function to initialize test tracing
pub fn setup_test_tracing() -> TestTracingSubscriber {
    let subscriber = TestTracingSubscriber::new();
    
    let _ = tracing_subscriber::registry()
        .with(subscriber.clone())
        .try_init();
    
    subscriber
}

/// Macro for asserting logs contain specific content
#[macro_export]
macro_rules! assert_log_contains {
    ($subscriber:expr, $level:expr, $pattern:expr) => {
        $subscriber.assert_log_contains($level, $pattern)
    };
}

/// Macro for asserting spans exist
#[macro_export]
macro_rules! assert_span_exists {
    ($subscriber:expr, $span_name:expr) => {
        $subscriber.assert_span_exists($span_name)
    };
}

/// Macro for asserting span fields
#[macro_export]
macro_rules! assert_span_field {
    ($subscriber:expr, $span_name:expr, $field_name:expr, $field_value:expr) => {
        $subscriber.assert_span_contains_field($span_name, $field_name, $field_value)
    };
}