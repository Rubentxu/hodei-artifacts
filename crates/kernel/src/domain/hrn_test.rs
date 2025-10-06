//! Tests exhaustivos para el módulo HRN (Hodei Resource Name)
//!
//! Este archivo contiene tests completos que cubren:
//! - Creación de HRNs válidos
//! - Validación de componentes
//! - Parsing de strings
//! - Serialización/deserialización
//! - Conversión a string
//! - Edge cases y errores
//! - Comparación e igualdad

#[cfg(test)]
mod hrn_tests {
    use crate::domain::hrn::Hrn;
    use serde_json;

    // ============================================================================
    // Tests de Creación
    // ============================================================================

    #[test]
    fn test_hrn_new_with_all_fields_succeeds() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        assert_eq!(hrn.partition(), "aws");
        assert_eq!(hrn.service(), "iam");
        assert_eq!(hrn.account_id(), "123456789012");
        assert_eq!(hrn.resource_type(), "User");
        assert_eq!(hrn.resource_id(), "alice");
    }

    #[test]
    fn test_hrn_new_with_default_account() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "organizations".to_string(),
            "default".to_string(),
            "Account".to_string(),
            "account-123".to_string(),
        );

        assert_eq!(hrn.account_id(), "default");
    }

    #[test]
    fn test_hrn_new_with_complex_resource_id() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "s3".to_string(),
            "123456789012".to_string(),
            "bucket".to_string(),
            "my-bucket-with-dashes-123".to_string(),
        );

        assert_eq!(hrn.resource_id(), "my-bucket-with-dashes-123");
    }

    #[test]
    fn test_hrn_new_with_uuid_resource_id() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "550e8400-e29b-41d4-a716-446655440000".to_string(),
        );

        assert_eq!(hrn.resource_id(), "550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn test_hrn_new_normalizes_service_name() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "IAM".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        // Service should be normalized to lowercase
        assert_eq!(hrn.service(), "iam");
    }

    // ============================================================================
    // Tests de Parsing
    // ============================================================================

    #[test]
    fn test_hrn_parse_valid_string_succeeds() {
        let hrn_string = "hrn:aws:iam::123456789012:User/alice";
        let hrn = Hrn::from_string(hrn_string).unwrap();

        assert_eq!(hrn.partition(), "aws");
        assert_eq!(hrn.service(), "iam");
        assert_eq!(hrn.account_id(), "123456789012");
        assert_eq!(hrn.resource_type(), "User");
        assert_eq!(hrn.resource_id(), "alice");
    }

    #[test]
    fn test_hrn_parse_with_slashes_in_resource_id() {
        let hrn_string = "hrn:aws:s3::123456789012:object/my-bucket/path/to/file.txt";
        let hrn = Hrn::from_string(hrn_string).unwrap();

        assert_eq!(hrn.resource_type(), "object");
        assert_eq!(hrn.resource_id(), "my-bucket/path/to/file.txt");
    }

    #[test]
    fn test_hrn_parse_malformed_string_fails() {
        let invalid_hrn = "invalid-hrn-format";
        let result = Hrn::from_string(invalid_hrn);

        assert!(
            result.is_none(),
            "Should fail to parse malformed HRN: {}",
            invalid_hrn
        );
    }

    #[test]
    fn test_hrn_parse_missing_prefix_fails() {
        let invalid_hrn = "aws:iam::123456789012:User/alice";
        let result = Hrn::from_string(invalid_hrn);

        assert!(
            result.is_none(),
            "Should fail without 'hrn:' prefix: {}",
            invalid_hrn
        );
    }

    #[test]
    fn test_hrn_parse_insufficient_components_fails() {
        let invalid_hrn = "hrn:aws:iam";
        let result = Hrn::from_string(invalid_hrn);

        assert!(
            result.is_none(),
            "Should fail with insufficient components: {}",
            invalid_hrn
        );
    }

    #[test]
    fn test_hrn_parse_only_prefix_fails() {
        let invalid_hrn = "hrn:";
        let result = Hrn::from_string(invalid_hrn);

        assert!(
            result.is_none(),
            "Should fail with only prefix: {}",
            invalid_hrn
        );
    }

    // ============================================================================
    // Tests de Conversión a String
    // ============================================================================

    #[test]
    fn test_hrn_to_string_format_correct() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let hrn_string = hrn.to_string();
        assert_eq!(hrn_string, "hrn:aws:iam::123456789012:User/alice");
    }

    #[test]
    fn test_hrn_to_string_preserves_slashes_in_resource_id() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "s3".to_string(),
            "123456789012".to_string(),
            "object".to_string(),
            "my-bucket/path/to/file.txt".to_string(),
        );

        let hrn_string = hrn.to_string();
        assert_eq!(
            hrn_string,
            "hrn:aws:s3::123456789012:object/my-bucket/path/to/file.txt"
        );
    }

    #[test]
    fn test_hrn_display_trait() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "s3".to_string(),
            "123456789012".to_string(),
            "bucket".to_string(),
            "my-bucket".to_string(),
        );

        let displayed = format!("{}", hrn);
        assert_eq!(displayed, "hrn:aws:s3::123456789012:bucket/my-bucket");
    }

    // ============================================================================
    // Tests de Roundtrip (Parse -> ToString -> Parse)
    // ============================================================================

    #[test]
    fn test_hrn_parse_to_string_roundtrip() {
        let original = "hrn:aws:iam::123456789012:User/alice";
        let hrn = Hrn::from_string(original).unwrap();
        let roundtrip = hrn.to_string();

        assert_eq!(original, roundtrip);
    }

    #[test]
    fn test_hrn_new_to_string_to_parse_roundtrip() {
        let hrn1 = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let hrn_string = hrn1.to_string();
        let hrn2 = Hrn::from_string(&hrn_string).unwrap();

        assert_eq!(hrn1, hrn2);
    }

    // ============================================================================
    // Tests de Igualdad y Comparación
    // ============================================================================

    #[test]
    fn test_hrn_equality_works() {
        let hrn1 = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let hrn2 = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        assert_eq!(hrn1, hrn2);
    }

    #[test]
    fn test_hrn_inequality_different_partition() {
        let hrn1 = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let hrn2 = Hrn::new(
            "gcp".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        assert_ne!(hrn1, hrn2);
    }

    #[test]
    fn test_hrn_inequality_different_resource_id() {
        let hrn1 = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let hrn2 = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "bob".to_string(),
        );

        assert_ne!(hrn1, hrn2);
    }

    #[test]
    fn test_hrn_clone_works() {
        let hrn1 = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let hrn2 = hrn1.clone();

        assert_eq!(hrn1, hrn2);
        assert_eq!(hrn1.partition(), hrn2.partition());
        assert_eq!(hrn1.service(), hrn2.service());
    }

    // ============================================================================
    // Tests de Serialización
    // ============================================================================

    #[test]
    fn test_hrn_serialization() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let serialized = serde_json::to_string(&hrn).expect("Should serialize");
        assert!(serialized.contains("aws"));
        assert!(serialized.contains("iam"));
        assert!(serialized.contains("alice"));
    }

    #[test]
    fn test_hrn_deserialization() {
        let json = r#"{"partition":"aws","service":"iam","account_id":"123456789012","resource_type":"User","resource_id":"alice"}"#;
        let hrn: Hrn = serde_json::from_str(json).expect("Should deserialize");

        assert_eq!(hrn.partition(), "aws");
        assert_eq!(hrn.service(), "iam");
        assert_eq!(hrn.resource_id(), "alice");
    }

    #[test]
    fn test_hrn_serialization_deserialization_roundtrip() {
        let hrn1 = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let json = serde_json::to_string(&hrn1).expect("Should serialize");
        let hrn2: Hrn = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(hrn1, hrn2);
    }

    // ============================================================================
    // Tests de Edge Cases
    // ============================================================================

    #[test]
    fn test_hrn_with_numeric_resource_id() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "123456".to_string(),
        );

        assert_eq!(hrn.resource_id(), "123456");
    }

    #[test]
    fn test_hrn_with_special_characters_in_resource_id() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "s3".to_string(),
            "123456789012".to_string(),
            "object".to_string(),
            "file-name_with.special@chars#123".to_string(),
        );

        assert_eq!(hrn.resource_id(), "file-name_with.special@chars#123");
    }

    #[test]
    fn test_hrn_with_very_long_resource_id() {
        let long_id = "a".repeat(500);
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            long_id.clone(),
        );

        assert_eq!(hrn.resource_id(), &long_id);
    }

    #[test]
    fn test_hrn_with_uppercase_service_normalized() {
        let hrn = Hrn::new(
            "AWS".to_string(),
            "IAM".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "Alice".to_string(),
        );

        // Service should be normalized to lowercase
        assert_eq!(hrn.service(), "iam");
        // Other fields preserve case
        assert_eq!(hrn.partition(), "AWS");
        assert_eq!(hrn.resource_id(), "Alice");
    }

    // ============================================================================
    // Tests de Componentes Individuales
    // ============================================================================

    #[test]
    fn test_hrn_partition_accessor() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        assert_eq!(hrn.partition(), "aws");
    }

    #[test]
    fn test_hrn_service_accessor() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "s3".to_string(),
            "123456789012".to_string(),
            "bucket".to_string(),
            "my-bucket".to_string(),
        );

        assert_eq!(hrn.service(), "s3");
    }

    #[test]
    fn test_hrn_account_id_accessor() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "ec2".to_string(),
            "123456789012".to_string(),
            "instance".to_string(),
            "i-1234567890abcdef".to_string(),
        );

        assert_eq!(hrn.account_id(), "123456789012");
    }

    #[test]
    fn test_hrn_resource_type_accessor() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "Group".to_string(),
            "developers".to_string(),
        );

        assert_eq!(hrn.resource_type(), "Group");
    }

    #[test]
    fn test_hrn_resource_id_accessor() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "admin-user-123".to_string(),
        );

        assert_eq!(hrn.resource_id(), "admin-user-123");
    }

    // ============================================================================
    // Tests de Debug
    // ============================================================================

    #[test]
    fn test_hrn_debug_format() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let debug_string = format!("{:?}", hrn);
        // Debug format should contain the HRN components
        assert!(debug_string.contains("aws") || debug_string.contains("iam"));
    }

    // ============================================================================
    // Tests de Conversión a Pascal Case (helper público)
    // ============================================================================

    #[test]
    fn test_to_pascal_case_simple() {
        let result = Hrn::to_pascal_case("iam");
        assert_eq!(result, "Iam");
    }

    #[test]
    fn test_to_pascal_case_with_hyphen() {
        let result = Hrn::to_pascal_case("my-service");
        assert_eq!(result, "MyService");
    }

    #[test]
    fn test_to_pascal_case_with_underscore() {
        let result = Hrn::to_pascal_case("my_service");
        assert_eq!(result, "MyService");
    }

    #[test]
    fn test_to_pascal_case_already_pascal() {
        let result = Hrn::to_pascal_case("MyService");
        assert_eq!(result, "Myservice");
    }

    #[test]
    fn test_to_pascal_case_empty_string() {
        let result = Hrn::to_pascal_case("");
        assert_eq!(result, "");
    }

    #[test]
    fn test_to_pascal_case_multiple_separators() {
        let result = Hrn::to_pascal_case("my-complex_service-name");
        assert_eq!(result, "MyComplexServiceName");
    }

    // ============================================================================
    // Tests de Casos Reales de Uso
    // ============================================================================

    #[test]
    fn test_hrn_for_iam_user() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        assert_eq!(hrn.to_string(), "hrn:aws:iam::123456789012:User/alice");
    }

    #[test]
    fn test_hrn_for_iam_group() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "Group".to_string(),
            "developers".to_string(),
        );

        assert_eq!(
            hrn.to_string(),
            "hrn:aws:iam::123456789012:Group/developers"
        );
    }

    #[test]
    fn test_hrn_for_organization_account() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "organizations".to_string(),
            "default".to_string(),
            "Account".to_string(),
            "123456789012".to_string(),
        );

        assert_eq!(
            hrn.to_string(),
            "hrn:aws:organizations::default:Account/123456789012"
        );
    }

    #[test]
    fn test_hrn_for_scp() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "organizations".to_string(),
            "default".to_string(),
            "ServiceControlPolicy".to_string(),
            "scp-123".to_string(),
        );

        assert_eq!(
            hrn.to_string(),
            "hrn:aws:organizations::default:ServiceControlPolicy/scp-123"
        );
    }

    #[test]
    fn test_hrn_for_s3_bucket() {
        let hrn = Hrn::new(
            "aws".to_string(),
            "s3".to_string(),
            "123456789012".to_string(),
            "bucket".to_string(),
            "my-application-bucket".to_string(),
        );

        assert_eq!(
            hrn.to_string(),
            "hrn:aws:s3::123456789012:bucket/my-application-bucket"
        );
    }

    // ============================================================================
    // Tests de Hash
    // ============================================================================

    #[test]
    fn test_hrn_can_be_used_in_hashmap() {
        use std::collections::HashMap;

        let hrn1 = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let hrn2 = hrn1.clone();

        let mut map = HashMap::new();
        map.insert(hrn1, "value1");

        assert_eq!(map.get(&hrn2), Some(&"value1"));
    }

    #[test]
    fn test_hrn_hash_consistency() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let hrn1 = Hrn::new(
            "aws".to_string(),
            "iam".to_string(),
            "123456789012".to_string(),
            "User".to_string(),
            "alice".to_string(),
        );

        let hrn2 = hrn1.clone();

        let mut hasher1 = DefaultHasher::new();
        hrn1.hash(&mut hasher1);
        let hash1 = hasher1.finish();

        let mut hasher2 = DefaultHasher::new();
        hrn2.hash(&mut hasher2);
        let hash2 = hasher2.finish();

        assert_eq!(hash1, hash2);
    }
}
