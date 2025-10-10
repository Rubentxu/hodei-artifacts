//! Context Converter Adapter for Playground Evaluate
//!
//! This adapter implements the ContextConverterPort trait by converting
//! playground attribute values to Cedar's RestrictedExpression format.

use super::super::dto::AttributeValue;
use super::super::error::PlaygroundEvaluateError;
use super::super::ports::ContextConverterPort;
use cedar_policy::RestrictedExpression;
use std::collections::HashMap;
use std::str::FromStr;
use tracing::{debug, warn};

/// Adapter that implements ContextConverterPort for attribute conversion
///
/// This adapter converts playground AttributeValue types to Cedar's
/// RestrictedExpression format, enabling context attributes to be used
/// in policy evaluation.
///
/// # Supported Types
///
/// - String: Converted to Cedar string
/// - Long: Converted to Cedar long (i64)
/// - Bool: Converted to Cedar boolean
/// - EntityRef: Converted to Cedar EntityUid
/// - Set: Recursively converted to Cedar set
/// - Record: Recursively converted to Cedar record
pub struct ContextConverterAdapter;

impl ContextConverterAdapter {
    /// Create a new context converter adapter
    pub fn new() -> Self {
        Self
    }

    /// Convert a single AttributeValue to RestrictedExpression
    ///
    /// # Arguments
    ///
    /// * `value` - The attribute value to convert
    ///
    /// # Returns
    ///
    /// A Cedar RestrictedExpression
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - EntityRef HRN is invalid
    /// - Nested conversion fails
    /// - Record creation fails
    #[allow(clippy::only_used_in_recursion)]
    fn convert_value(
        &self,
        value: &AttributeValue,
    ) -> Result<RestrictedExpression, PlaygroundEvaluateError> {
        match value {
            AttributeValue::String(s) => {
                debug!("Converting string attribute");
                Ok(RestrictedExpression::new_string(s.clone()))
            }
            AttributeValue::Long(n) => {
                debug!("Converting long attribute");
                Ok(RestrictedExpression::new_long(*n))
            }
            AttributeValue::Bool(b) => {
                debug!("Converting bool attribute");
                Ok(RestrictedExpression::new_bool(*b))
            }
            AttributeValue::EntityRef(hrn) => {
                debug!(hrn = %hrn, "Converting EntityRef attribute");
                let entity_uid_string = hrn.entity_uid_string();
                let entity_uid =
                    cedar_policy::EntityUid::from_str(&entity_uid_string).map_err(|e| {
                        warn!(hrn = %hrn, error = %e, "Failed to convert HRN to EntityUid");
                        PlaygroundEvaluateError::InvalidContextAttribute(format!(
                            "Invalid EntityRef HRN '{}': {}",
                            hrn, e
                        ))
                    })?;
                Ok(RestrictedExpression::new_entity_uid(entity_uid))
            }
            AttributeValue::Set(values) => {
                debug!(count = values.len(), "Converting set attribute");
                let converted: Result<Vec<_>, _> =
                    values.iter().map(|v| self.convert_value(v)).collect();
                let converted_values = converted?;
                Ok(RestrictedExpression::new_set(converted_values))
            }
            AttributeValue::Record(map) => {
                debug!(count = map.len(), "Converting record attribute");
                let mut converted_map = HashMap::new();
                for (key, value) in map {
                    let converted_value = self.convert_value(value)?;
                    converted_map.insert(key.clone(), converted_value);
                }
                RestrictedExpression::new_record(converted_map).map_err(|e| {
                    warn!(error = %e, "Failed to create record");
                    PlaygroundEvaluateError::InvalidContextAttribute(format!(
                        "Failed to create record: {}",
                        e
                    ))
                })
            }
        }
    }
}

impl Default for ContextConverterAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextConverterPort for ContextConverterAdapter {
    fn convert_context(
        &self,
        attributes: &HashMap<String, AttributeValue>,
    ) -> Result<HashMap<String, RestrictedExpression>, PlaygroundEvaluateError> {
        debug!(
            attribute_count = attributes.len(),
            "Converting context attributes"
        );

        let mut converted = HashMap::new();

        for (key, value) in attributes {
            debug!(key = %key, "Converting attribute");
            let converted_value = self.convert_value(value)?;
            converted.insert(key.clone(), converted_value);
        }

        debug!(
            converted_count = converted.len(),
            "Context conversion complete"
        );

        Ok(converted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::Hrn;

    #[test]
    fn test_convert_string() {
        let converter = ContextConverterAdapter::new();
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            AttributeValue::String("Alice".to_string()),
        );

        let result = converter.convert_context(&attrs);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn test_convert_long() {
        let converter = ContextConverterAdapter::new();
        let mut attrs = HashMap::new();
        attrs.insert("age".to_string(), AttributeValue::Long(42));

        let result = converter.convert_context(&attrs);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn test_convert_bool() {
        let converter = ContextConverterAdapter::new();
        let mut attrs = HashMap::new();
        attrs.insert("active".to_string(), AttributeValue::Bool(true));

        let result = converter.convert_context(&attrs);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn test_convert_entity_ref() {
        let converter = ContextConverterAdapter::new();
        let mut attrs = HashMap::new();
        let hrn = Hrn::new(
            "hodei".to_string(),
            "iam".to_string(),
            "default".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );
        attrs.insert("owner".to_string(), AttributeValue::EntityRef(hrn));

        let result = converter.convert_context(&attrs);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn test_convert_set() {
        let converter = ContextConverterAdapter::new();
        let mut attrs = HashMap::new();
        attrs.insert(
            "tags".to_string(),
            AttributeValue::Set(vec![
                AttributeValue::String("tag1".to_string()),
                AttributeValue::String("tag2".to_string()),
            ]),
        );

        let result = converter.convert_context(&attrs);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn test_convert_record() {
        let converter = ContextConverterAdapter::new();
        let mut inner_map = HashMap::new();
        inner_map.insert(
            "city".to_string(),
            AttributeValue::String("NYC".to_string()),
        );
        inner_map.insert("zip".to_string(), AttributeValue::Long(10001));

        let mut attrs = HashMap::new();
        attrs.insert("address".to_string(), AttributeValue::Record(inner_map));

        let result = converter.convert_context(&attrs);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn test_convert_empty_context() {
        let converter = ContextConverterAdapter::new();
        let attrs = HashMap::new();

        let result = converter.convert_context(&attrs);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[test]
    fn test_convert_multiple_attributes() {
        let converter = ContextConverterAdapter::new();
        let mut attrs = HashMap::new();
        attrs.insert(
            "name".to_string(),
            AttributeValue::String("Alice".to_string()),
        );
        attrs.insert("age".to_string(), AttributeValue::Long(30));
        attrs.insert("active".to_string(), AttributeValue::Bool(true));

        let result = converter.convert_context(&attrs);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 3);
    }

    #[test]
    fn test_default_constructor() {
        let _converter = ContextConverterAdapter;
    }
}
