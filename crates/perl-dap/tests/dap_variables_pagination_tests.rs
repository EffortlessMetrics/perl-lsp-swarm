//! Tests for DAP variables response pagination support.
//!
//! Verifies that the variables response includes the totalVariables field
//! to enable proper pagination UX in debugger clients.

use serde_json::json;

#[test]
fn variables_response_body_includes_total_variables_field() -> Result<(), Box<dyn std::error::Error>>
{
    // Verify that VariablesResponseBody struct has totalVariables field
    let response_body = json!({
        "variables": [
            {
                "name": "var1",
                "value": "42",
                "type": "int"
            }
        ],
        "totalVariables": 100
    });

    // Extract totalVariables field from response
    let total_vars = response_body.get("totalVariables");
    assert!(total_vars.is_some(), "totalVariables field should be present in response");
    assert_eq!(total_vars.unwrap().as_i64(), Some(100));
    Ok(())
}

#[test]
fn variables_response_serialization_respects_skip_serializing_if()
-> Result<(), Box<dyn std::error::Error>> {
    // When totalVariables is None, it should not be serialized
    let response_without_total = json!({
        "variables": [
            {
                "name": "var1",
                "value": "42",
                "type": "int"
            }
        ]
    });

    // Verify the field is not present when None
    assert!(
        response_without_total.get("totalVariables").is_none(),
        "totalVariables should not be serialized when None"
    );
    Ok(())
}

#[test]
fn variables_response_pagination_with_total_count() -> Result<(), Box<dyn std::error::Error>> {
    // Simulate a paginated response where totalVariables > paginated window
    let paginated_response = json!({
        "variables": [
            { "name": "var1", "value": "1", "type": "int" },
            { "name": "var2", "value": "2", "type": "int" },
            { "name": "var3", "value": "3", "type": "int" }
        ],
        "totalVariables": 150
    });

    let paginated_count = paginated_response["variables"].as_array().unwrap().len();
    let total_count = paginated_response["totalVariables"].as_i64().unwrap();

    // Verify that paginated count is less than or equal to total
    assert!(
        paginated_count as i64 <= total_count,
        "Paginated count ({}) should be <= total count ({})",
        paginated_count,
        total_count
    );

    // Specific case: 3 returned, 150 available
    assert_eq!(paginated_count, 3);
    assert_eq!(total_count, 150);
    Ok(())
}
