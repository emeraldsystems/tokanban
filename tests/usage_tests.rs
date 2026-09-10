/// Tests for the `tokanban usage` command's API interactions and response
/// deserialization, against the wiremock harness.
mod common;

use common::mock_server::MockServer;
use serde_json::json;
use tokanban::commands::usage::{UsageBreakdown, UsageSummary};

fn summary_json() -> serde_json::Value {
    json!({
        "from": 1765000000,
        "to": 1765600000,
        "sessions": 5,
        "input_tokens": 120000,
        "output_tokens": 30000,
        "cache_read_tokens": 500000,
        "cache_write_tokens": 0,
        "total_tokens": 650000,
        "estimated_cost_usd": 2.75,
        "active_now": 2
    })
}

fn breakdown_json() -> serde_json::Value {
    json!({
        "from": 1765000000,
        "to": 1765600000,
        "by": "model",
        "groups": [
            {
                "key": "claude-opus-4-8",
                "sessions": 4,
                "input_tokens": 100000,
                "output_tokens": 25000,
                "cache_read_tokens": 500000,
                "cache_write_tokens": 0,
                "estimated_cost_usd": 2.75
            },
            {
                "key": "mystery-model",
                "sessions": 1,
                "input_tokens": 20000,
                "output_tokens": 5000,
                "cache_read_tokens": 0,
                "cache_write_tokens": 0,
                "estimated_cost_usd": null
            }
        ]
    })
}

#[tokio::test]
async fn test_usage_summary_deserializes() {
    let server = MockServer::start().await;
    server.mock_get("/v1/usage/summary", summary_json()).await;

    let client =
        tokanban::api::ApiClient::new(&server.base_url(), 30, Some("tk_test".to_string())).unwrap();
    let summary: UsageSummary = client.get("/v1/usage/summary?range=30d").await.unwrap();

    assert_eq!(summary.sessions, 5);
    assert_eq!(summary.total_tokens, Some(650_000));
    assert_eq!(summary.active_now, 2);
    assert!((summary.estimated_cost_usd.unwrap() - 2.75).abs() < 1e-9);
}

#[tokio::test]
async fn test_usage_breakdown_deserializes_with_unknown_cost() {
    let server = MockServer::start().await;
    server
        .mock_get("/v1/usage/breakdown", breakdown_json())
        .await;

    let client =
        tokanban::api::ApiClient::new(&server.base_url(), 30, Some("tk_test".to_string())).unwrap();
    let breakdown: UsageBreakdown = client
        .get("/v1/usage/breakdown?by=model&range=30d")
        .await
        .unwrap();

    assert_eq!(breakdown.by, "model");
    assert_eq!(breakdown.groups.len(), 2);
    let opus = &breakdown.groups[0];
    assert_eq!(opus.key.as_deref(), Some("claude-opus-4-8"));
    assert_eq!(opus.sessions, 4);
    assert!(opus.estimated_cost_usd.is_some());
    let mystery = &breakdown.groups[1];
    assert!(mystery.estimated_cost_usd.is_none());
}
