use crate::features::validate_policy::dto::*;
use std::collections::HashMap;

/// Enhanced error formatter for policy validation errors
pub struct PolicyValidationErrorFormatter {
    suggestion_templates: HashMap<ValidationErrorType, Vec<String>>,
    documentation_links: HashMap<ValidationErrorType, String>,
}

impl PolicyValidationErrorFormatter {
    pub fn new() -> Self {
        let mut formatter = Self {
            suggestion_templates: HashMap::new(),
            documentation_links: HashMap::new(),
        };
        
        formatter.initialize_templates();
        formatter.initialize_documentation_links();
        formatter
    }

    /// Format a validation error with enhanced information
    pub fn format_error(&self, error_type: ValidationErrorType, message: &str, location: Option<PolicyLocation>) -> ValidationError {
        let suggested_fix = self.generate_suggestion(&error_type, message);
        let documentation_link = self.documentation_links.get(&error_type).cloned();
        let enhanced_message = self.enhance_message(&error_type, message, location.as_ref());

        ValidationError {
            error_type,
            message: enhanced_message,
            location,
            suggested_fix,
            documentation_link,
        }
    }

    /// Format multiple errors with context and grouping
    pub fn format_errors_with_context(&self, errors: Vec<(ValidationErrorType, String, Option<PolicyLocation>)>) -> Vec<ValidationError> {
        let mut formatted_errors = Vec::new();
        let mut error_counts = HashMap::new();

        // Count error types for better reporting
        for (error_type, _, _) in &errors {
            *error_counts.entry(error_type.clone()).or_insert(0) += 1;
        }

        for (error_type, message, location) in errors {
            let mut formatted_error = self.format_error(error_type.clone(), &message, location);
            
            // Add context based on error frequency
            if let Some(count) = error_counts.get(&error_type) {
                if *count > 1 {
                    formatted_error.message = format!(
                        "{} (This is one of {} similar {} errors)",
                        formatted_error.message,
                        count,
                        self.error_type_name(&error_type)
                    );
                }
            }

            formatted_errors.push(formatted_error);
        }

        // Sort errors by severity and location
        formatted_errors.sort_by(|a, b| {
            // First by error type severity
            let severity_a = self.get_error_severity(&a.error_type);
            let severity_b = self.get_error_severity(&b.error_type);
            
            match severity_a.cmp(&severity_b) {
                std::cmp::Ordering::Equal => {
                    // Then by location
                    match (&a.location, &b.location) {
                        (Some(loc_a), Some(loc_b)) => {
                            loc_a.line.cmp(&loc_b.line).then(loc_a.column.cmp(&loc_b.column))
                        }
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (None, None) => std::cmp::Ordering::Equal,
                    }
                }
                other => other,
            }
        });

        formatted_errors
    }

    /// Generate contextual suggestions based on error type and message
    fn generate_suggestion(&self, error_type: &ValidationErrorType, message: &str) -> Option<String> {
        let templates = self.suggestion_templates.get(error_type)?;
        
        // Use simple pattern matching to select appropriate suggestion
        for template in templates {
            if self.matches_error_pattern(template, message) {
                return Some(self.customize_suggestion(template, message));
            }
        }

        // Return first template as fallback
        templates.first().cloned()
    }

    /// Enhance error message with location and context information
    fn enhance_message(&self, error_type: &ValidationErrorType, message: &str, location: Option<&PolicyLocation>) -> String {
        let mut enhanced = message.to_string();

        // Add location information if available
        if let Some(loc) = location {
            enhanced = format!("Line {}, Column {}: {}", loc.line, loc.column, enhanced);
        }

        // Add error type context
        let error_context = match error_type {
            ValidationErrorType::SyntaxError => "This is a Cedar policy syntax error.",
            ValidationErrorType::SemanticError => "This error indicates the policy doesn't match the expected schema.",
            ValidationErrorType::HrnError => "This error is related to Hodei Resource Name (HRN) format or validation.",
            ValidationErrorType::SchemaViolation => "The policy violates the defined schema constraints.",
            ValidationErrorType::TypeMismatch => "There's a type mismatch in the policy expression.",
            ValidationErrorType::UnknownEntity => "The policy references an entity type that doesn't exist in the schema.",
            ValidationErrorType::UnknownAction => "The policy references an action that doesn't exist in the schema.",
            ValidationErrorType::UnknownAttribute => "The policy references an attribute that doesn't exist for this entity type.",
            ValidationErrorType::ConstraintViolation => "The policy violates a defined constraint.",
        };

        format!("{} {}", enhanced, error_context)
    }

    /// Initialize suggestion templates for different error types
    fn initialize_templates(&mut self) {
        self.suggestion_templates.insert(
            ValidationErrorType::SyntaxError,
            vec![
                "Check for missing semicolons, parentheses, or quotes in your policy.".to_string(),
                "Ensure all Cedar policy keywords are spelled correctly (permit, forbid, when, unless).".to_string(),
                "Verify that all strings are properly quoted and escaped.".to_string(),
                "Check that all parentheses and brackets are properly balanced.".to_string(),
            ],
        );

        self.suggestion_templates.insert(
            ValidationErrorType::UnknownEntity,
            vec![
                "Check the entity type name against the schema. Available entity types can be found in the schema documentation.".to_string(),
                "Ensure the entity type is properly namespaced (e.g., 'MyApp::User' instead of just 'User').".to_string(),
                "Verify that the entity type exists in the current schema version.".to_string(),
            ],
        );

        self.suggestion_templates.insert(
            ValidationErrorType::UnknownAction,
            vec![
                "Verify the action name against the schema. Check available actions in the schema documentation.".to_string(),
                "Ensure the action is properly formatted (e.g., 'read', 'write', 'delete').".to_string(),
                "Check if the action requires specific context or conditions.".to_string(),
            ],
        );

        self.suggestion_templates.insert(
            ValidationErrorType::UnknownAttribute,
            vec![
                "Check that the attribute exists for this entity type in the schema.".to_string(),
                "Verify the attribute name spelling and case sensitivity.".to_string(),
                "Ensure you're accessing the attribute on the correct entity (principal, resource, etc.).".to_string(),
            ],
        );

        self.suggestion_templates.insert(
            ValidationErrorType::TypeMismatch,
            vec![
                "Check that you're comparing values of compatible types (string with string, number with number).".to_string(),
                "Ensure attribute access returns the expected type according to the schema.".to_string(),
                "Verify that function calls return the correct type for the context.".to_string(),
            ],
        );

        self.suggestion_templates.insert(
            ValidationErrorType::HrnError,
            vec![
                "Ensure HRN follows the format: hrn:partition:service:region:account:resource-type/resource-id".to_string(),
                "Check that all HRN components are properly formatted and non-empty.".to_string(),
                "Verify that the resource type and ID are valid for your use case.".to_string(),
            ],
        );

        self.suggestion_templates.insert(
            ValidationErrorType::SchemaViolation,
            vec![
                "Review the schema constraints and ensure your policy complies with them.".to_string(),
                "Check required attributes and their expected values.".to_string(),
                "Verify that optional attributes are used correctly.".to_string(),
            ],
        );

        self.suggestion_templates.insert(
            ValidationErrorType::ConstraintViolation,
            vec![
                "Review the constraint definition and adjust your policy accordingly.".to_string(),
                "Check if the constraint requires specific conditions or context.".to_string(),
                "Ensure all required fields are present and properly formatted.".to_string(),
            ],
        );

        self.suggestion_templates.insert(
            ValidationErrorType::SemanticError,
            vec![
                "Review the policy logic and ensure it makes semantic sense.".to_string(),
                "Check that all referenced entities, actions, and attributes exist in the schema.".to_string(),
                "Verify that conditions and expressions are logically consistent.".to_string(),
            ],
        );
    }

    /// Initialize documentation links for different error types
    fn initialize_documentation_links(&mut self) {
        let base_url = "https://docs.hodei.com/iam/policies";
        
        self.documentation_links.insert(
            ValidationErrorType::SyntaxError,
            format!("{}/syntax", base_url),
        );
        
        self.documentation_links.insert(
            ValidationErrorType::SemanticError,
            format!("{}/semantics", base_url),
        );
        
        self.documentation_links.insert(
            ValidationErrorType::HrnError,
            format!("{}/hrn-format", base_url),
        );
        
        self.documentation_links.insert(
            ValidationErrorType::SchemaViolation,
            format!("{}/schema", base_url),
        );
        
        self.documentation_links.insert(
            ValidationErrorType::UnknownEntity,
            format!("{}/entities", base_url),
        );
        
        self.documentation_links.insert(
            ValidationErrorType::UnknownAction,
            format!("{}/actions", base_url),
        );
        
        self.documentation_links.insert(
            ValidationErrorType::UnknownAttribute,
            format!("{}/attributes", base_url),
        );
        
        self.documentation_links.insert(
            ValidationErrorType::TypeMismatch,
            format!("{}/types", base_url),
        );
        
        self.documentation_links.insert(
            ValidationErrorType::ConstraintViolation,
            format!("{}/constraints", base_url),
        );
    }

    /// Check if a suggestion template matches the error pattern
    fn matches_error_pattern(&self, _template: &str, _message: &str) -> bool {
        // Simple implementation - in a real system, this would use more sophisticated pattern matching
        true
    }

    /// Customize suggestion based on specific error message
    fn customize_suggestion(&self, template: &str, message: &str) -> String {
        // Extract specific information from error message and customize template
        if message.contains("unknown entity") {
            if let Some(entity_name) = self.extract_entity_name(message) {
                return format!("The entity type '{}' is not defined in the schema. {}", entity_name, template);
            }
        }
        
        if message.contains("unknown action") {
            if let Some(action_name) = self.extract_action_name(message) {
                return format!("The action '{}' is not defined in the schema. {}", action_name, template);
            }
        }

        template.to_string()
    }

    /// Extract entity name from error message
    fn extract_entity_name(&self, message: &str) -> Option<String> {
        // Simple regex-like extraction - in practice, use proper regex
        if let Some(start) = message.find("entity '") {
            let start = start + 8; // Length of "entity '"
            if let Some(end) = message[start..].find('\'') {
                return Some(message[start..start + end].to_string());
            }
        }
        None
    }

    /// Extract action name from error message
    fn extract_action_name(&self, message: &str) -> Option<String> {
        // Simple regex-like extraction - in practice, use proper regex
        if let Some(start) = message.find("action '") {
            let start = start + 8; // Length of "action '"
            if let Some(end) = message[start..].find('\'') {
                return Some(message[start..start + end].to_string());
            }
        }
        None
    }

    /// Get error type name for display
    fn error_type_name(&self, error_type: &ValidationErrorType) -> &'static str {
        match error_type {
            ValidationErrorType::SyntaxError => "syntax",
            ValidationErrorType::SemanticError => "semantic",
            ValidationErrorType::HrnError => "HRN",
            ValidationErrorType::SchemaViolation => "schema violation",
            ValidationErrorType::TypeMismatch => "type mismatch",
            ValidationErrorType::UnknownEntity => "unknown entity",
            ValidationErrorType::UnknownAction => "unknown action",
            ValidationErrorType::UnknownAttribute => "unknown attribute",
            ValidationErrorType::ConstraintViolation => "constraint violation",
        }
    }

    /// Get error severity for sorting (lower number = higher severity)
    fn get_error_severity(&self, error_type: &ValidationErrorType) -> u8 {
        match error_type {
            ValidationErrorType::SyntaxError => 1,
            ValidationErrorType::SchemaViolation => 2,
            ValidationErrorType::SemanticError => 3,
            ValidationErrorType::UnknownEntity => 4,
            ValidationErrorType::UnknownAction => 5,
            ValidationErrorType::UnknownAttribute => 6,
            ValidationErrorType::TypeMismatch => 7,
            ValidationErrorType::ConstraintViolation => 8,
            ValidationErrorType::HrnError => 9,
        }
    }
}

impl Default for PolicyValidationErrorFormatter {
    fn default() -> Self {
        Self::new()
    }
}

/// Enhanced warning formatter for policy validation warnings
pub struct PolicyValidationWarningFormatter;

impl PolicyValidationWarningFormatter {
    pub fn new() -> Self {
        Self
    }

    /// Format a validation warning with enhanced information
    pub fn format_warning(&self, message: &str, location: Option<PolicyLocation>, severity: WarningSeverity) -> ValidationWarning {
        let enhanced_message = self.enhance_warning_message(message, location.as_ref(), &severity);
        
        ValidationWarning {
            message: enhanced_message,
            location,
            severity,
        }
    }

    /// Enhance warning message with context
    fn enhance_warning_message(&self, message: &str, location: Option<&PolicyLocation>, severity: &WarningSeverity) -> String {
        let mut enhanced = message.to_string();

        // Add location information if available
        if let Some(loc) = location {
            enhanced = format!("Line {}, Column {}: {}", loc.line, loc.column, enhanced);
        }

        // Add severity context
        let severity_context = match severity {
            WarningSeverity::High => "⚠️  HIGH PRIORITY",
            WarningSeverity::Medium => "⚠️  MEDIUM PRIORITY",
            WarningSeverity::Low => "ℹ️  LOW PRIORITY",
        };

        format!("{} - {}", severity_context, enhanced)
    }
}

impl Default for PolicyValidationWarningFormatter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_formatter_creation() {
        let formatter = PolicyValidationErrorFormatter::new();
        assert!(!formatter.suggestion_templates.is_empty());
        assert!(!formatter.documentation_links.is_empty());
    }

    #[test]
    fn test_format_syntax_error() {
        let formatter = PolicyValidationErrorFormatter::new();
        let location = PolicyLocation { line: 5, column: 10 };
        
        let error = formatter.format_error(
            ValidationErrorType::SyntaxError,
            "Missing semicolon",
            Some(location),
        );

        assert!(matches!(error.error_type, ValidationErrorType::SyntaxError));
        assert!(error.message.contains("Line 5, Column 10"));
        assert!(error.message.contains("syntax error"));
        assert!(error.suggested_fix.is_some());
        assert!(error.documentation_link.is_some());
    }

    #[test]
    fn test_format_unknown_entity_error() {
        let formatter = PolicyValidationErrorFormatter::new();
        
        let error = formatter.format_error(
            ValidationErrorType::UnknownEntity,
            "unknown entity 'MyApp::InvalidEntity'",
            None,
        );

        assert!(matches!(error.error_type, ValidationErrorType::UnknownEntity));
        assert!(error.suggested_fix.is_some());
        let suggestion = error.suggested_fix.unwrap();
        assert!(suggestion.contains("MyApp::InvalidEntity"));
    }

    #[test]
    fn test_format_errors_with_context() {
        let formatter = PolicyValidationErrorFormatter::new();
        
        let errors = vec![
            (ValidationErrorType::SyntaxError, "Error 1".to_string(), None),
            (ValidationErrorType::SyntaxError, "Error 2".to_string(), None),
            (ValidationErrorType::UnknownEntity, "Error 3".to_string(), None),
        ];

        let formatted = formatter.format_errors_with_context(errors);
        
        assert_eq!(formatted.len(), 3);
        // Syntax errors should be first (higher severity)
        assert!(matches!(formatted[0].error_type, ValidationErrorType::SyntaxError));
        assert!(matches!(formatted[1].error_type, ValidationErrorType::SyntaxError));
        // Should mention multiple similar errors
        assert!(formatted[0].message.contains("one of 2 similar"));
    }

    #[test]
    fn test_warning_formatter() {
        let formatter = PolicyValidationWarningFormatter::new();
        let location = PolicyLocation { line: 3, column: 5 };
        
        let warning = formatter.format_warning(
            "This condition might be redundant",
            Some(location),
            WarningSeverity::Medium,
        );

        assert!(warning.message.contains("Line 3, Column 5"));
        assert!(warning.message.contains("MEDIUM PRIORITY"));
        assert!(matches!(warning.severity, WarningSeverity::Medium));
    }

    #[test]
    fn test_extract_entity_name() {
        let formatter = PolicyValidationErrorFormatter::new();
        let message = "unknown entity 'MyApp::User' in policy";
        
        let entity_name = formatter.extract_entity_name(message);
        assert_eq!(entity_name, Some("MyApp::User".to_string()));
    }

    #[test]
    fn test_extract_action_name() {
        let formatter = PolicyValidationErrorFormatter::new();
        let message = "unknown action 'read_secret' in policy";
        
        let action_name = formatter.extract_action_name(message);
        assert_eq!(action_name, Some("read_secret".to_string()));
    }

    #[test]
    fn test_error_severity_ordering() {
        let formatter = PolicyValidationErrorFormatter::new();
        
        let syntax_severity = formatter.get_error_severity(&ValidationErrorType::SyntaxError);
        let semantic_severity = formatter.get_error_severity(&ValidationErrorType::SemanticError);
        let hrn_severity = formatter.get_error_severity(&ValidationErrorType::HrnError);
        
        assert!(syntax_severity < semantic_severity);
        assert!(semantic_severity < hrn_severity);
    }
}