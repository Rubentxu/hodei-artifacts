use super::error_formatter::*;
use super::dto::*;

#[test]
fn test_error_formatter_initialization() {
    let formatter = PolicyValidationErrorFormatter::new();
    
    // Verify that templates and documentation links are initialized
    assert!(!formatter.suggestion_templates.is_empty());
    assert!(!formatter.documentation_links.is_empty());
    
    // Check that all error types have templates
    assert!(formatter.suggestion_templates.contains_key(&ValidationErrorType::SyntaxError));
    assert!(formatter.suggestion_templates.contains_key(&ValidationErrorType::SemanticError));
    assert!(formatter.suggestion_templates.contains_key(&ValidationErrorType::UnknownEntity));
    assert!(formatter.suggestion_templates.contains_key(&ValidationErrorType::UnknownAction));
}

#[test]
fn test_format_syntax_error_with_location() {
    let formatter = PolicyValidationErrorFormatter::new();
    let location = PolicyLocation { line: 10, column: 25 };
    
    let error = formatter.format_error(
        ValidationErrorType::SyntaxError,
        "Unexpected token ';'",
        Some(location),
    );
    
    assert!(matches!(error.error_type, ValidationErrorType::SyntaxError));
    assert!(error.message.contains("Line 10, Column 25"));
    assert!(error.message.contains("syntax error"));
    assert!(error.suggested_fix.is_some());
    assert!(error.documentation_link.is_some());
    
    let suggestion = error.suggested_fix.unwrap();
    assert!(!suggestion.is_empty());
    
    let doc_link = error.documentation_link.unwrap();
    assert!(doc_link.contains("syntax"));
}

#[test]
fn test_format_unknown_entity_error() {
    let formatter = PolicyValidationErrorFormatter::new();
    
    let error = formatter.format_error(
        ValidationErrorType::UnknownEntity,
        "unknown entity 'MyApp::InvalidEntity' in policy",
        None,
    );
    
    assert!(matches!(error.error_type, ValidationErrorType::UnknownEntity));
    assert!(error.suggested_fix.is_some());
    
    let suggestion = error.suggested_fix.unwrap();
    assert!(suggestion.contains("MyApp::InvalidEntity"));
    assert!(suggestion.contains("entity type"));
}

#[test]
fn test_format_unknown_action_error() {
    let formatter = PolicyValidationErrorFormatter::new();
    
    let error = formatter.format_error(
        ValidationErrorType::UnknownAction,
        "unknown action 'invalid_action' in policy",
        Some(PolicyLocation { line: 5, column: 15 }),
    );
    
    assert!(matches!(error.error_type, ValidationErrorType::UnknownAction));
    assert!(error.message.contains("Line 5, Column 15"));
    assert!(error.suggested_fix.is_some());
    
    let suggestion = error.suggested_fix.unwrap();
    assert!(suggestion.contains("invalid_action"));
}

#[test]
fn test_format_multiple_errors_with_context() {
    let formatter = PolicyValidationErrorFormatter::new();
    
    let errors = vec![
        (ValidationErrorType::SyntaxError, "Error 1".to_string(), Some(PolicyLocation { line: 1, column: 1 })),
        (ValidationErrorType::SyntaxError, "Error 2".to_string(), Some(PolicyLocation { line: 2, column: 1 })),
        (ValidationErrorType::SemanticError, "Error 3".to_string(), Some(PolicyLocation { line: 3, column: 1 })),
        (ValidationErrorType::UnknownEntity, "Error 4".to_string(), None),
    ];
    
    let formatted = formatter.format_errors_with_context(errors);
    
    assert_eq!(formatted.len(), 4);
    
    // Check that syntax errors are first (higher priority)
    assert!(matches!(formatted[0].error_type, ValidationErrorType::SyntaxError));
    assert!(matches!(formatted[1].error_type, ValidationErrorType::SyntaxError));
    
    // Check that multiple syntax errors are noted
    assert!(formatted[0].message.contains("one of 2 similar"));
    assert!(formatted[1].message.contains("one of 2 similar"));
    
    // Check location ordering within same error type
    assert!(formatted[0].location.as_ref().unwrap().line < formatted[1].location.as_ref().unwrap().line);
}

#[test]
fn test_error_severity_ordering() {
    let formatter = PolicyValidationErrorFormatter::new();
    
    let syntax_severity = formatter.get_error_severity(&ValidationErrorType::SyntaxError);
    let semantic_severity = formatter.get_error_severity(&ValidationErrorType::SemanticError);
    let unknown_entity_severity = formatter.get_error_severity(&ValidationErrorType::UnknownEntity);
    let hrn_severity = formatter.get_error_severity(&ValidationErrorType::HrnError);
    
    // Syntax errors should have highest priority (lowest number)
    assert!(syntax_severity < semantic_severity);
    assert!(semantic_severity < unknown_entity_severity);
    assert!(unknown_entity_severity < hrn_severity);
}

#[test]
fn test_extract_entity_name_variations() {
    let formatter = PolicyValidationErrorFormatter::new();
    
    // Test different entity name formats
    let test_cases = vec![
        ("unknown entity 'User' in policy", Some("User".to_string())),
        ("unknown entity 'MyApp::User' in policy", Some("MyApp::User".to_string())),
        ("entity 'Complex::Nested::Entity' not found", Some("Complex::Nested::Entity".to_string())),
        ("no entity mentioned here", None),
    ];
    
    for (message, expected) in test_cases {
        let result = formatter.extract_entity_name(message);
        assert_eq!(result, expected, "Failed for message: {}", message);
    }
}

#[test]
fn test_extract_action_name_variations() {
    let formatter = PolicyValidationErrorFormatter::new();
    
    let test_cases = vec![
        ("unknown action 'read' in policy", Some("read".to_string())),
        ("unknown action 'complex_action_name' in policy", Some("complex_action_name".to_string())),
        ("action 'MyApp::Action' not found", Some("MyApp::Action".to_string())),
        ("no action mentioned here", None),
    ];
    
    for (message, expected) in test_cases {
        let result = formatter.extract_action_name(message);
        assert_eq!(result, expected, "Failed for message: {}", message);
    }
}

#[test]
fn test_customize_suggestion_for_entity_error() {
    let formatter = PolicyValidationErrorFormatter::new();
    
    let template = "Check the entity type name against the schema.";
    let message = "unknown entity 'InvalidEntity' in policy";
    
    let customized = formatter.customize_suggestion(template, message);
    
    assert!(customized.contains("InvalidEntity"));
    assert!(customized.contains("entity type"));
    assert!(customized.contains("schema"));
}

#[test]
fn test_customize_suggestion_for_action_error() {
    let formatter = PolicyValidationErrorFormatter::new();
    
    let template = "Verify the action name against the schema.";
    let message = "unknown action 'invalid_action' in policy";
    
    let customized = formatter.customize_suggestion(template, message);
    
    assert!(customized.contains("invalid_action"));
    assert!(customized.contains("action"));
    assert!(customized.contains("schema"));
}

#[test]
fn test_error_type_names() {
    let formatter = PolicyValidationErrorFormatter::new();
    
    assert_eq!(formatter.error_type_name(&ValidationErrorType::SyntaxError), "syntax");
    assert_eq!(formatter.error_type_name(&ValidationErrorType::SemanticError), "semantic");
    assert_eq!(formatter.error_type_name(&ValidationErrorType::UnknownEntity), "unknown entity");
    assert_eq!(formatter.error_type_name(&ValidationErrorType::UnknownAction), "unknown action");
    assert_eq!(formatter.error_type_name(&ValidationErrorType::HrnError), "HRN");
}

#[test]
fn test_warning_formatter() {
    let formatter = PolicyValidationWarningFormatter::new();
    
    let warning = formatter.format_warning(
        "This condition might be redundant",
        Some(PolicyLocation { line: 5, column: 10 }),
        WarningSeverity::High,
    );
    
    assert!(warning.message.contains("Line 5, Column 10"));
    assert!(warning.message.contains("HIGH PRIORITY"));
    assert!(warning.message.contains("⚠️"));
    assert!(matches!(warning.severity, WarningSeverity::High));
}

#[test]
fn test_warning_formatter_different_severities() {
    let formatter = PolicyValidationWarningFormatter::new();
    
    let high_warning = formatter.format_warning(
        "Critical issue",
        None,
        WarningSeverity::High,
    );
    
    let medium_warning = formatter.format_warning(
        "Medium issue",
        None,
        WarningSeverity::Medium,
    );
    
    let low_warning = formatter.format_warning(
        "Minor issue",
        None,
        WarningSeverity::Low,
    );
    
    assert!(high_warning.message.contains("HIGH PRIORITY"));
    assert!(medium_warning.message.contains("MEDIUM PRIORITY"));
    assert!(low_warning.message.contains("LOW PRIORITY"));
    assert!(low_warning.message.contains("ℹ️"));
}

#[test]
fn test_documentation_links_completeness() {
    let formatter = PolicyValidationErrorFormatter::new();
    
    // Verify all error types have documentation links
    let error_types = vec![
        ValidationErrorType::SyntaxError,
        ValidationErrorType::SemanticError,
        ValidationErrorType::HrnError,
        ValidationErrorType::SchemaViolation,
        ValidationErrorType::UnknownEntity,
        ValidationErrorType::UnknownAction,
        ValidationErrorType::UnknownAttribute,
        ValidationErrorType::TypeMismatch,
        ValidationErrorType::ConstraintViolation,
    ];
    
    for error_type in error_types {
        assert!(formatter.documentation_links.contains_key(&error_type),
                "Missing documentation link for {:?}", error_type);
        
        let link = formatter.documentation_links.get(&error_type).unwrap();
        assert!(link.starts_with("https://docs.hodei.com/iam/policies"));
    }
}

#[test]
fn test_suggestion_templates_completeness() {
    let formatter = PolicyValidationErrorFormatter::new();
    
    // Verify all error types have suggestion templates
    let error_types = vec![
        ValidationErrorType::SyntaxError,
        ValidationErrorType::SemanticError,
        ValidationErrorType::HrnError,
        ValidationErrorType::SchemaViolation,
        ValidationErrorType::UnknownEntity,
        ValidationErrorType::UnknownAction,
        ValidationErrorType::UnknownAttribute,
        ValidationErrorType::TypeMismatch,
        ValidationErrorType::ConstraintViolation,
    ];
    
    for error_type in error_types {
        assert!(formatter.suggestion_templates.contains_key(&error_type),
                "Missing suggestion templates for {:?}", error_type);
        
        let templates = formatter.suggestion_templates.get(&error_type).unwrap();
        assert!(!templates.is_empty(), "Empty templates for {:?}", error_type);
        
        // Verify all templates are non-empty
        for template in templates {
            assert!(!template.trim().is_empty(), "Empty template found for {:?}", error_type);
        }
    }
}

#[test]
fn test_default_implementations() {
    let formatter1 = PolicyValidationErrorFormatter::default();
    let formatter2 = PolicyValidationErrorFormatter::new();
    
    // Both should have the same number of templates and links
    assert_eq!(formatter1.suggestion_templates.len(), formatter2.suggestion_templates.len());
    assert_eq!(formatter1.documentation_links.len(), formatter2.documentation_links.len());
    
    let warning_formatter1 = PolicyValidationWarningFormatter::default();
    let warning_formatter2 = PolicyValidationWarningFormatter::new();
    
    // Both should work the same way
    let warning1 = warning_formatter1.format_warning("test", None, WarningSeverity::Medium);
    let warning2 = warning_formatter2.format_warning("test", None, WarningSeverity::Medium);
    
    assert_eq!(warning1.message, warning2.message);
}