#[cfg(test)]
mod http_error_conversion_tests {
    use crate::code::ErrorCode;
    use crate::error::{ErrorDetail, ErrorOutput, HTTPError, TryFromHTTPError};
    use rpc_error_convert::HTTPErrorConversion;
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use std::convert::TryFrom;

    // Test data structures
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub struct TestUserData {
        pub id: u64,
        pub username: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub struct ValidationDetails {
        pub field: String,
        pub message: String,
    }

    // Test enum demonstrating all supported field types
    #[derive(
        Debug, Clone, thiserror::Error, HTTPErrorConversion, Serialize, Deserialize, PartialEq,
    )]
    pub enum TestAppError {
        // Unit variant - no data
        #[bad_request("generic-error")]
        #[error("generic error occurred")]
        GenericError,

        // Single unnamed field - data passed directly
        #[not_found("user-not-found")]
        #[error("user not found: {0:?}")]
        UserNotFound(TestUserData),

        // Multiple unnamed fields - generates TestAppErrorMultipleFieldsData tuple struct
        #[bad_request("validation-failed")]
        #[error("validation failed")]
        ValidationFailed(String, u32, bool),

        // Named fields - generates TestAppErrorDuplicateUserData struct
        #[already_exists("duplicate-user")]
        #[error("duplicate user with id {id} and email {email}")]
        DuplicateUser {
            /// User ID that already exists
            id: u64,
            /// Email address associated with the duplicate user
            email: String,
        },

        // Complex named fields with optional - generates TestAppErrorComplexErrorData struct
        #[failed_precondition("complex-error")]
        #[error("complex error")]
        ComplexError {
            /// Error code for the complex error
            code: i32,
            /// Whether the system is active
            active: bool,
            /// Optional additional details about the error
            details: Option<String>,
            /// Optional validation metadata
            metadata: Option<ValidationDetails>,
        },
    }

    #[test]
    fn test_unit_variant_conversion() {
        let error = TestAppError::GenericError;
        let http_error: HTTPError = error.into();

        assert_eq!(http_error.code, ErrorCode::BadRequest);
        assert_eq!(http_error.reason, "generic-error");
        assert!(http_error.data.is_none());

        // Convert back
        let http_error = HTTPError::new(
            ErrorCode::BadRequest,
            "generic-error",
            None::<Box<dyn std::error::Error>>,
            None::<()>,
        );
        let recovered = TestAppError::try_from(http_error).unwrap();
        assert_eq!(recovered, TestAppError::GenericError);
    }

    #[test]
    fn test_single_unnamed_field_conversion() {
        let user_data = TestUserData {
            id: 42,
            username: "testuser".to_string(),
        };
        let error = TestAppError::UserNotFound(user_data.clone());
        let http_error: HTTPError = error.into();

        assert_eq!(http_error.code, ErrorCode::NotFound);
        assert_eq!(http_error.reason, "user-not-found");
        assert!(http_error.data.is_some());

        // Verify serialized data
        let data_value = http_error.data.unwrap();
        let deserialized: TestUserData = serde_json::from_value(data_value.clone()).unwrap();
        assert_eq!(deserialized, user_data);

        // Convert back
        let http_error = HTTPError::new(
            ErrorCode::NotFound,
            "user-not-found",
            None::<Box<dyn std::error::Error>>,
            Some(data_value),
        );
        let recovered = TestAppError::try_from(http_error).unwrap();
        assert_eq!(recovered, TestAppError::UserNotFound(user_data));
    }

    #[test]
    fn test_multiple_unnamed_fields_conversion() {
        let error = TestAppError::ValidationFailed("email field".to_string(), 400, true);
        let http_error: HTTPError = error.clone().into();

        assert_eq!(http_error.code, ErrorCode::BadRequest);
        assert_eq!(http_error.reason, "validation-failed");
        assert!(http_error.data.is_some());

        // Verify the data is serialized as a tuple array
        let data_value = http_error.data.unwrap();
        let array: Vec<serde_json::Value> = serde_json::from_value(data_value.clone()).unwrap();
        assert_eq!(array.len(), 3);
        assert_eq!(array[0], json!("email field"));
        assert_eq!(array[1], json!(400));
        assert_eq!(array[2], json!(true));

        // Convert back
        let http_error = HTTPError::new(
            ErrorCode::BadRequest,
            "validation-failed",
            None::<Box<dyn std::error::Error>>,
            Some(data_value),
        );
        let recovered = TestAppError::try_from(http_error).unwrap();
        assert_eq!(recovered, error);
    }

    #[test]
    fn test_named_fields_conversion() {
        let error = TestAppError::DuplicateUser {
            id: 999,
            email: "test@example.com".to_string(),
        };
        let http_error: HTTPError = error.clone().into();

        assert_eq!(http_error.code, ErrorCode::AlreadyExists);
        assert_eq!(http_error.reason, "duplicate-user");
        assert!(http_error.data.is_some());

        // Verify the data is serialized as an object
        let data_value = http_error.data.unwrap();
        assert_eq!(
            data_value,
            json!({
                "id": 999,
                "email": "test@example.com"
            })
        );

        // Convert back
        let http_error = HTTPError::new(
            ErrorCode::AlreadyExists,
            "duplicate-user",
            None::<Box<dyn std::error::Error>>,
            Some(data_value),
        );
        let recovered = TestAppError::try_from(http_error).unwrap();
        assert_eq!(recovered, error);
    }

    #[test]
    fn test_complex_named_fields_with_optionals() {
        let error = TestAppError::ComplexError {
            code: -100,
            active: false,
            details: Some("Additional context".to_string()),
            metadata: Some(ValidationDetails {
                field: "username".to_string(),
                message: "Too short".to_string(),
            }),
        };
        let http_error: HTTPError = error.clone().into();

        assert_eq!(http_error.code, ErrorCode::FailedPrecondition);
        assert_eq!(http_error.reason, "complex-error");
        assert!(http_error.data.is_some());

        let data_value = http_error.data.unwrap();
        let expected = json!({
            "code": -100,
            "active": false,
            "details": "Additional context",
            "metadata": {
                "field": "username",
                "message": "Too short"
            }
        });
        assert_eq!(data_value, expected);

        // Convert back
        let http_error = HTTPError::new(
            ErrorCode::FailedPrecondition,
            "complex-error",
            None::<Box<dyn std::error::Error>>,
            Some(data_value),
        );
        let recovered = TestAppError::try_from(http_error).unwrap();
        assert_eq!(recovered, error);
    }

    #[test]
    fn test_complex_named_fields_with_none_values() {
        let error = TestAppError::ComplexError {
            code: 50,
            active: true,
            details: None,
            metadata: None,
        };
        let http_error: HTTPError = error.clone().into();

        let data_value = http_error.data.unwrap();
        assert_eq!(
            data_value,
            json!({
                "code": 50,
                "active": true,
                "details": null,
                "metadata": null
            })
        );

        // Convert back with null values
        let http_error = HTTPError::new(
            ErrorCode::FailedPrecondition,
            "complex-error",
            None::<Box<dyn std::error::Error>>,
            Some(data_value),
        );
        let recovered = TestAppError::try_from(http_error).unwrap();
        assert_eq!(recovered, error);
    }

    #[test]
    fn test_from_error_output() {
        let error_output = ErrorOutput {
            error: ErrorDetail {
                code: ErrorCode::AlreadyExists,
                reason: "duplicate-user".to_string(),
                message: "User already exists".to_string(),
                data: Some(json!({
                    "id": 777,
                    "email": "output@example.com"
                })),
            },
        };

        let result = TestAppError::try_from(error_output);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            TestAppError::DuplicateUser {
                id: 777,
                email: "output@example.com".to_string(),
            }
        );
    }

    #[test]
    fn test_missing_data_error() {
        let http_error = HTTPError::new(
            ErrorCode::NotFound,
            "user-not-found",
            None::<Box<dyn std::error::Error>>,
            None::<()>,
        );

        let result = TestAppError::try_from(http_error);
        assert!(result.is_err());
        match result.unwrap_err() {
            TryFromHTTPError::MissingData => {}
            _ => panic!("Expected MissingData error"),
        }
    }

    #[test]
    fn test_unknown_reason_error() {
        let http_error = HTTPError::new(
            ErrorCode::BadRequest,
            "unknown-error-code",
            None::<Box<dyn std::error::Error>>,
            None::<()>,
        );

        let result = TestAppError::try_from(http_error);
        assert!(result.is_err());
        match result.unwrap_err() {
            TryFromHTTPError::UnknownReason(reason) => {
                assert_eq!(reason, "unknown-error-code");
            }
            _ => panic!("Expected UnknownReason error"),
        }
    }

    #[test]
    fn test_deserialization_error() {
        // Create HTTPError with invalid data for the expected type
        let http_error = HTTPError::new(
            ErrorCode::NotFound,
            "user-not-found",
            None::<Box<dyn std::error::Error>>,
            Some(json!("invalid_data_type")), // String instead of expected object
        );

        let result = TestAppError::try_from(http_error);
        assert!(result.is_err());
        match result.unwrap_err() {
            TryFromHTTPError::DeserializationError => {}
            _ => panic!("Expected DeserializationError"),
        }
    }

    // Test to ensure generated structs exist and work correctly
    #[test]
    fn test_generated_structs() {
        // ValidationFailedData should be generated as a tuple struct
        let validation_data = ValidationFailedData("test".to_string(), 123, false);
        let json_value = serde_json::to_value(&validation_data).unwrap();
        let deserialized: ValidationFailedData = serde_json::from_value(json_value).unwrap();
        assert_eq!(deserialized.0, "test");
        assert_eq!(deserialized.1, 123);
        assert!(!deserialized.2);

        // DuplicateUserData should be generated as a named struct
        let user_data = DuplicateUserData {
            id: 456,
            email: "struct@test.com".to_string(),
        };
        let json_value = serde_json::to_value(&user_data).unwrap();
        let deserialized: DuplicateUserData = serde_json::from_value(json_value).unwrap();
        assert_eq!(deserialized.id, 456);
        assert_eq!(deserialized.email, "struct@test.com");

        // ComplexErrorData should be generated with optional fields
        let complex_data = ComplexErrorData {
            code: 789,
            active: true,
            details: None,
            metadata: Some(ValidationDetails {
                field: "test".to_string(),
                message: "msg".to_string(),
            }),
        };
        let json_value = serde_json::to_value(&complex_data).unwrap();
        let deserialized: ComplexErrorData = serde_json::from_value(json_value).unwrap();
        assert_eq!(deserialized.code, 789);
        assert!(deserialized.active);
        assert_eq!(deserialized.details, None);
        assert!(deserialized.metadata.is_some());
    }
}
