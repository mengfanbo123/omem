use crate::domain::error::OmemError;

#[async_trait::async_trait]
pub trait LlmService: Send + Sync {
    async fn complete_text(&self, system: &str, user: &str) -> Result<String, OmemError>;
}

/// Strips thinking tags (`<think>` and `</think>`) and markdown fences from LLM output.
pub fn strip_markdown_fences(s: &str) -> &str {
    let trimmed = s.trim();

    // Step 1: Strip thinking tags first (for reasoning models like MiniMax-M2.7)
    // Handle both standard and multiline thinking tags
    let without_thinking = strip_thinking_tags(trimmed);

    // Step 2: Strip markdown fences
    if let Some(rest) = without_thinking.strip_prefix("```json") {
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim();
        }
    }

    if let Some(rest) = without_thinking.strip_prefix("```") {
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim();
        }
    }

    without_thinking.trim()
}

/// Strips `<think>...</think>` and `<think>...` tags from LLM output.
fn strip_thinking_tags(s: &str) -> &str {
    // Handle standard thinking tags: <think>...</think>
    if let Some(start) = s.find("<think>") {
        if let Some(end) = s.find("</think>") {
            let before = &s[..start];
            let after = &s[end + "</think>".len()..];
            return strip_thinking_tags(format!("{before}{after}").as_str());
        }
    }
    // Handle unclosed thinking tag (defensive)
    if let Some(start) = s.find("<think>") {
        let before = &s[..start];
        let after = &s[start + "<think>".len()..];
        return strip_thinking_tags(format!("{before}{after}").as_str());
    }
    s
}

/// Complete a prompt and parse the response as typed JSON.
/// Retries once with an error hint on parse failure.
pub async fn complete_json<T: serde::de::DeserializeOwned>(
    llm: &dyn LlmService,
    system: &str,
    user: &str,
) -> Result<T, OmemError> {
    let text = llm.complete_text(system, user).await?;
    let cleaned = strip_markdown_fences(&text);

    match serde_json::from_str(cleaned) {
        Ok(v) => Ok(v),
        Err(_first_err) => {
            let retry_user = format!(
                "{user}\n\nYour previous response was not valid JSON. Return ONLY valid JSON."
            );
            let text = llm.complete_text(system, &retry_user).await?;
            let cleaned = strip_markdown_fences(&text);
            serde_json::from_str(cleaned)
                .map_err(|e| OmemError::Llm(format!("JSON parse failed after retry: {e}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_json_fence() {
        let input = "```json\n{\"key\": \"value\"}\n```";
        assert_eq!(strip_markdown_fences(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn strip_plain_fence() {
        let input = "```\n{\"a\": 1}\n```";
        assert_eq!(strip_markdown_fences(input), "{\"a\": 1}");
    }

    #[test]
    fn no_fence_passthrough() {
        let input = "{\"key\": \"value\"}";
        assert_eq!(strip_markdown_fences(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn strip_with_whitespace() {
        let input = "  \n```json\n  {\"x\": true}  \n```\n  ";
        assert_eq!(strip_markdown_fences(input), "{\"x\": true}");
    }

    #[test]
    fn strip_fence_no_newline_after_lang() {
        let input = "```json{\"y\": 42}```";
        assert_eq!(strip_markdown_fences(input), "{\"y\": 42}");
    }

    #[test]
    fn already_clean_json() {
        let input = "  {\"hello\": \"world\"}  ";
        assert_eq!(strip_markdown_fences(input), "{\"hello\": \"world\"}");
    }

    #[test]
    fn strip_multiline_json() {
        let input = "```json\n{\n  \"items\": [\n    1,\n    2\n  ]\n}\n```";
        let result = strip_markdown_fences(input);
        assert!(result.starts_with('{'));
        assert!(result.ends_with('}'));
        assert!(result.contains("\"items\""));
    }

    #[test]
    fn strip_thinking_tags() {
        let input = "<think>
Let me analyze the facts carefully.
</think>
{"key": "value"}";
        assert_eq!(strip_markdown_fences(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn strip_thinking_tags_multiline() {
        let input = "<think>
The user is a developer.
I need to extract facts.
</think>
{"memories": [{"l0_abstract": "User is a developer"}]}</think>";
        assert_eq!(
            strip_markdown_fences(input),
            "{\"memories\": [{\"l0_abstract\": \"User is a developer\"}]}"
        );
    }

    #[test]
    fn strip_thinking_tags_no_closer() {
        let input = "<think>
This thinking never ends
{"key": "value"}";
        assert_eq!(strip_markdown_fences(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn strip_thinking_tags_with_json_fence() {
        let input = "<think>
Analyzing...
</think>
```json
{"key": "value"}
```</think>";
        assert_eq!(strip_markdown_fences(input), "{\"key\": \"value\"}");
    }
}
