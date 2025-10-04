use cedar_policy::RestrictedExpression;
use std::collections::{BTreeMap, HashMap};

/// Helper trait for converting common Rust types to RestrictedExpression
///
/// This trait provides convenient methods for converting standard Rust types
/// to RestrictedExpression values that can be used as entity attributes.
pub trait ToRestrictedExpression {
    /// Convert the value to a RestrictedExpression
    fn to_restricted_expr(&self) -> RestrictedExpression;
}

impl ToRestrictedExpression for String {
    fn to_restricted_expr(&self) -> RestrictedExpression {
        RestrictedExpression::new_string(self.clone())
    }
}

impl ToRestrictedExpression for &str {
    fn to_restricted_expr(&self) -> RestrictedExpression {
        RestrictedExpression::new_string(self.to_string())
    }
}

impl ToRestrictedExpression for bool {
    fn to_restricted_expr(&self) -> RestrictedExpression {
        RestrictedExpression::new_bool(*self)
    }
}

impl ToRestrictedExpression for i64 {
    fn to_restricted_expr(&self) -> RestrictedExpression {
        RestrictedExpression::new_long(*self)
    }
}

impl ToRestrictedExpression for i32 {
    fn to_restricted_expr(&self) -> RestrictedExpression {
        RestrictedExpression::new_long(*self as i64)
    }
}

impl<T: ToRestrictedExpression> ToRestrictedExpression for Vec<T> {
    fn to_restricted_expr(&self) -> RestrictedExpression {
        let expressions: Vec<RestrictedExpression> =
            self.iter().map(|item| item.to_restricted_expr()).collect();
        RestrictedExpression::new_set(expressions)
    }
}

impl<K, V> ToRestrictedExpression for HashMap<K, V>
where
    K: AsRef<str>,
    V: ToRestrictedExpression,
{
    fn to_restricted_expr(&self) -> RestrictedExpression {
        let map: BTreeMap<String, RestrictedExpression> = self
            .iter()
            .map(|(k, v)| (k.as_ref().to_string(), v.to_restricted_expr()))
            .collect();
        RestrictedExpression::new_record(map).unwrap_or_else(|_| {
            RestrictedExpression::new_string("error_creating_record".to_string())
        })
    }
}

impl<K, V> ToRestrictedExpression for BTreeMap<K, V>
where
    K: AsRef<str>,
    V: ToRestrictedExpression,
{
    fn to_restricted_expr(&self) -> RestrictedExpression {
        let map: BTreeMap<String, RestrictedExpression> = self
            .iter()
            .map(|(k, v)| (k.as_ref().to_string(), v.to_restricted_expr()))
            .collect();
        RestrictedExpression::new_record(map).unwrap_or_else(|_| {
            RestrictedExpression::new_string("error_creating_record".to_string())
        })
    }
}

/// Builder for creating entity attributes map
///
/// This provides a fluent API for building the attributes map required by HodeiEntity.
///
/// # Example
///
/// ```
/// use policies::domain::entity_utils::AttributesBuilder;
///
/// let attributes = AttributesBuilder::new()
///     .attr("name", "Alice")
///     .attr("age", 30i64)
///     .attr("active", true)
///     .attr("tags", vec!["employee", "fulltime"])
///     .build();
/// ```
pub struct AttributesBuilder {
    attributes: HashMap<String, RestrictedExpression>,
}

impl AttributesBuilder {
    /// Create a new AttributesBuilder
    pub fn new() -> Self {
        Self {
            attributes: HashMap::new(),
        }
    }

    /// Add an attribute to the builder
    ///
    /// # Example
    ///
    /// ```
    /// use policies::domain::entity_utils::AttributesBuilder;
    ///
    /// let attributes = AttributesBuilder::new()
    ///     .attr("name", "Alice")
    ///     .attr("age", 30i64)
    ///     .build();
    /// ```
    pub fn attr<T: ToRestrictedExpression>(mut self, name: &str, value: T) -> Self {
        self.attributes
            .insert(name.to_string(), value.to_restricted_expr());
        self
    }

    /// Build the attributes map
    pub fn build(self) -> HashMap<String, RestrictedExpression> {
        self.attributes
    }
}

impl Default for AttributesBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_conversions() {
        let string_expr = "test".to_restricted_expr();
        // RestrictedExpression implements Debug, not Display
        assert!(format!("{:?}", string_expr).contains("test"));

        let bool_expr = true.to_restricted_expr();
        assert!(format!("{:?}", bool_expr).to_lowercase().contains("true"));

        let int_expr = 42i64.to_restricted_expr();
        assert!(format!("{:?}", int_expr).contains("42"));
    }

    #[test]
    fn test_collection_conversions() {
        let vec_expr = vec!["a", "b", "c"].to_restricted_expr();
        let vec_str = format!("{:?}", vec_expr);
        assert!(!vec_str.is_empty());

        let mut map = HashMap::new();
        map.insert("key1", "value1");
        map.insert("key2", "value2");
        let map_expr = map.to_restricted_expr();
        let map_str = format!("{:?}", map_expr);
        assert!(!map_str.is_empty());
    }

    #[test]
    fn test_attributes_builder() {
        let attributes = AttributesBuilder::new()
            .attr("name", "Alice")
            .attr("age", 30i64)
            .attr("active", true)
            .attr("tags", vec!["employee", "fulltime"])
            .build();

        assert_eq!(attributes.len(), 4);
        assert!(attributes.contains_key("name"));
        assert!(attributes.contains_key("age"));
        assert!(attributes.contains_key("active"));
        assert!(attributes.contains_key("tags"));
    }
}
