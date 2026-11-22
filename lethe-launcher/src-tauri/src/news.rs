//! Optional news feed. There is no bundled hardcoded news endpoint: the URL
//! comes from the active profile's `newsUrl`. An empty URL disables the feed.

use crate::profiles::NewsItem;

pub fn news_url() -> String {
    crate::profiles::active_profile().news_url
}

fn parse_items(body: &str) -> Result<Vec<NewsItem>, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("Invalid news response: {}", e))?;
    let list = value
        .as_array()
        .or_else(|| value.get("items").and_then(|v| v.as_array()))
        .or_else(|| value.get("news").and_then(|v| v.as_array()))
        .ok_or_else(|| "News response is not an array".to_string())?;
    serde_json::from_value(serde_json::Value::Array(list.clone()))
        .map_err(|e| format!("Invalid news item: {}", e))
}

/// Fetch the active profile's news items. Returns an empty list when the
/// profile has no news URL configured.
pub async fn fetch_news() -> Result<Vec<NewsItem>, String> {
    let url = news_url();
    if url.trim().is_empty() {
        return Ok(Vec::new());
    }
    let response = reqwest::get(&url)
        .await
        .map_err(|e| format!("Failed to fetch news: {}", e))?;
    if !response.status().is_success() {
        return Err(format!("News server returned HTTP {}", response.status()));
    }
    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read news: {}", e))?;
    parse_items(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_array() {
        let items = parse_items(r#"[{"title":"Hello","date":"2025-01-01"}]"#).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Hello");
    }

    #[test]
    fn test_parse_wrapped() {
        let items = parse_items(r#"{"items":[{"title":"A"}]}"#).unwrap();
        assert_eq!(items[0].title, "A");
    }

    #[test]
    fn test_parse_invalid() {
        assert!(parse_items("not json").is_err());
        assert!(parse_items(r#"{"other":[]}"#).is_err());
    }
}
