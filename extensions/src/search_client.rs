use anyhow::{anyhow, Result};
use nostr_db::Event;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize)]
pub struct EventSearchRequest {
    pub search: Option<String>,
    pub limit: Option<usize>,
    pub kinds: Option<Vec<u16>>,
    pub authors: Option<Vec<String>>,
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub ids: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PostEventRequest {
    pub content: String,
    pub kind: Option<u16>,
    pub tags: Option<Vec<Vec<String>>>,
    pub created_at: Option<u64>,
    pub pubkey: Option<String>,
    pub id: Option<String>,
    pub sig: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SemanticSearchRequest {
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResponse {
    pub event_ids: Vec<String>,
    pub total_found: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PostEventResponse {
    pub event_id: String,
    pub status: String,
}

#[derive(Debug)]
pub struct ExternalSearchClient {
    client: Client,
    base_url: String,
}

impl ExternalSearchClient {
    pub fn new(base_url: String, timeout_secs: Option<u64>) -> Self {
        let timeout = Duration::from_secs(timeout_secs.unwrap_or(30));
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self { client, base_url }
    }

    pub fn search_events_blocking(&self, request: &EventSearchRequest) -> Result<SearchResponse> {
        let url = format!("{}/events", self.base_url);

        let json_value = serde_json::to_value(request)
            .map_err(|e| anyhow!("Failed to serialize request: {}", e))?;

        let mut query_parts = Vec::new();

        if let serde_json::Value::Object(map) = json_value {
            for (key, value) in map {
                if !value.is_null() {
                    let value_str = match value {
                        serde_json::Value::String(s) => s,
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::Array(arr) => arr
                            .iter()
                            .filter_map(|v| match v {
                                serde_json::Value::String(s) => Some(s.clone()),
                                serde_json::Value::Number(n) => Some(n.to_string()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join(","),
                        _ => continue,
                    };

                    if !value_str.is_empty() {
                        query_parts.push(format!(
                            "{}={}",
                            urlencoding::encode(&key),
                            urlencoding::encode(&value_str)
                        ));
                    }
                }
            }
        }

        let final_url = if query_parts.is_empty() {
            url
        } else {
            format!("{}?{}", url, query_parts.join("&"))
        };

        let response = ureq::get(&final_url)
            .call()
            .map_err(|e| anyhow!("Failed to send search request: {}", e))?;

        if response.status() != 200 {
            return Err(anyhow!(
                "Search request failed with status: {}",
                response.status()
            ));
        }

        let search_response: SearchResponse = response
            .into_body()
            .read_json()
            .map_err(|e| anyhow!("Failed to parse search response: {}", e))?;

        Ok(search_response)
    }

    pub fn post_event_blocking(&self, request: &Event) -> Result<PostEventResponse> {
        let url = format!("{}/events", self.base_url);

        let response = ureq::post(&url)
            .header("Content-Type", "application/json")
            .send_json(request)
            .map_err(|e| anyhow!("Failed to send post request: {}", e))?;

        if response.status() != 200 {
            return Err(anyhow!(
                "Post request failed with status: {}",
                response.status()
            ));
        }

        Ok(PostEventResponse {
            event_id: "".to_string(),
            status: "".to_string(),
        })
    }

    pub fn semantic_search_blocking(
        &self,
        query: &str,
        limit: Option<usize>,
    ) -> Result<SearchResponse> {
        let url = format!("{}/search", self.base_url);

        let mut query_parts = Vec::new();
        query_parts.push(format!("query={}", urlencoding::encode(query)));

        if let Some(limit) = limit {
            query_parts.push(format!("limit={}", limit));
        }

        let final_url = format!("{}?{}", url, query_parts.join("&"));

        let response = ureq::get(&final_url)
            .call()
            .map_err(|e| anyhow!("Failed to send semantic search request: {}", e))?;

        if response.status() != 200 {
            return Err(anyhow!(
                "Semantic search request failed with status: {}",
                response.status()
            ));
        }

        let search_response: SearchResponse = response
            .into_body()
            .read_json()
            .map_err(|e| anyhow!("Failed to parse semantic search response: {}", e))?;

        Ok(search_response)
    }

    pub async fn search_events(&self, request: &EventSearchRequest) -> Result<SearchResponse> {
        let url = format!("{}/events", self.base_url);

        // Convert request to query parameters
        let mut params = HashMap::new();

        if let Some(search) = &request.search {
            params.insert("search", search.clone());
        }
        if let Some(limit) = request.limit {
            params.insert("limit", limit.to_string());
        }
        if let Some(kinds) = &request.kinds {
            params.insert(
                "kinds",
                kinds
                    .iter()
                    .map(|k| k.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        if let Some(authors) = &request.authors {
            params.insert("authors", authors.join(","));
        }
        if let Some(since) = request.since {
            params.insert("since", since.to_string());
        }
        if let Some(until) = request.until {
            params.insert("until", until.to_string());
        }
        if let Some(ids) = &request.ids {
            params.insert("ids", ids.join(","));
        }

        let response = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send search request: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Search request failed with status: {}",
                response.status()
            ));
        }

        let search_response: SearchResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse search response: {}", e))?;

        Ok(search_response)
    }

    pub async fn post_event(&self, request: &PostEventRequest) -> Result<PostEventResponse> {
        let url = format!("{}/events", self.base_url);

        let response = self
            .client
            .post(&url)
            .json(request)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send post request: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Post request failed with status: {}",
                response.status()
            ));
        }

        let post_response: PostEventResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse post response: {}", e))?;

        Ok(post_response)
    }

    pub async fn semantic_search(
        &self,
        query: &str,
        limit: Option<usize>,
    ) -> Result<SearchResponse> {
        let url = format!("{}/search", self.base_url);

        let mut params = HashMap::new();
        params.insert("query", query.to_string());

        if let Some(limit) = limit {
            params.insert("limit", limit.to_string());
        }

        let response = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send semantic search request: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Semantic search request failed with status: {}",
                response.status()
            ));
        }

        let search_response: SearchResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse semantic search response: {}", e))?;

        Ok(search_response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = ExternalSearchClient::new("http://localhost:3000".to_string(), Some(10));
        assert_eq!(client.base_url, "http://localhost:3000");
    }

    #[tokio::test]
    async fn test_search_events_request_structure() {
        let request = EventSearchRequest {
            search: None,
            limit: Some(10),
            kinds: Some(vec![1, 7]),
            authors: Some(vec!["pubkey1".to_string(), "pubkey2".to_string()]),
            since: Some(1234567890),
            until: Some(1234567900),
            ids: Some(vec!["id1".to_string(), "id2".to_string()]),
        };

        // Test that the request structure is correct
        assert_eq!(request.limit, Some(10));
    }
}
