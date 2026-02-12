//! Context Assembly — gathers relevant context before AI calls
//!
//! Before calling AI for task decomposition or execution:
//! 1. Query memory service for relevant patterns/procedures
//! 2. Query tool registry for available tools matching task
//! 3. Assemble system prompt with context

use anyhow::Result;
use tracing::debug;

/// Assembled context for an AI call
#[derive(Debug, Clone)]
pub struct AssembledContext {
    /// System prompt with role and capabilities
    pub system_prompt: String,
    /// Relevant memory/knowledge context
    pub memory_context: Vec<ContextChunk>,
    /// Available tools matching the task
    pub available_tools: Vec<String>,
    /// Total estimated token count
    pub estimated_tokens: i32,
}

/// A chunk of context from a memory tier
#[derive(Debug, Clone)]
pub struct ContextChunk {
    pub source: String,
    pub content: String,
    pub relevance: f64,
}

/// Assembles context for AI calls
pub struct ContextAssembler {
    max_context_tokens: i32,
}

impl ContextAssembler {
    pub fn new(max_context_tokens: i32) -> Self {
        Self { max_context_tokens }
    }

    /// Assemble context for a task
    ///
    /// In a full implementation, this would call the memory and tools
    /// gRPC services. For now, it assembles context from local state.
    pub fn assemble_for_task(
        &self,
        task_description: &str,
        intelligence_level: &str,
        available_patterns: &[(String, String, f64)], // (trigger, action, success_rate)
        tool_names: &[String],
    ) -> Result<AssembledContext> {
        let mut memory_context = Vec::new();
        let mut total_tokens = 0;

        // Add relevant patterns as context
        for (trigger, action, success_rate) in available_patterns {
            let content = format!(
                "Previously when '{}' occurred, action '{}' was taken with {:.0}% success rate",
                trigger, action, success_rate * 100.0
            );
            let tokens = estimate_tokens(&content);
            if total_tokens + tokens > self.max_context_tokens {
                break;
            }
            memory_context.push(ContextChunk {
                source: "patterns".to_string(),
                content,
                relevance: *success_rate,
            });
            total_tokens += tokens;
        }

        // Build system prompt
        let system_prompt = build_system_prompt(
            task_description,
            intelligence_level,
            tool_names,
        );
        total_tokens += estimate_tokens(&system_prompt);

        debug!(
            "Assembled context: {} chunks, ~{} tokens",
            memory_context.len(),
            total_tokens
        );

        Ok(AssembledContext {
            system_prompt,
            memory_context,
            available_tools: tool_names.to_vec(),
            estimated_tokens: total_tokens,
        })
    }
}

/// Build the system prompt for an AI call
fn build_system_prompt(
    task_description: &str,
    intelligence_level: &str,
    tool_names: &[String],
) -> String {
    let tools_list = if tool_names.is_empty() {
        "No specific tools required".to_string()
    } else {
        tool_names.join(", ")
    };

    format!(
        "You are aiOS, an AI-native operating system agent. \
         Your current task: {task_description}\n\
         Intelligence level: {intelligence_level}\n\
         Available tools: {tools_list}\n\n\
         Respond with a JSON object containing:\n\
         - \"steps\": array of task steps to execute\n\
         - \"tools_needed\": array of tool names to use\n\
         - \"reasoning\": brief explanation of your approach"
    )
}

/// Rough token estimation (4 chars per token)
fn estimate_tokens(text: &str) -> i32 {
    (text.len() as f64 / 4.0).ceil() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_assembler_new() {
        let assembler = ContextAssembler::new(4000);
        assert_eq!(assembler.max_context_tokens, 4000);
    }

    #[test]
    fn test_assemble_for_task_empty() {
        let assembler = ContextAssembler::new(4000);
        let ctx = assembler
            .assemble_for_task("Check system health", "reactive", &[], &[])
            .unwrap();

        assert!(!ctx.system_prompt.is_empty());
        assert!(ctx.memory_context.is_empty());
        assert!(ctx.available_tools.is_empty());
        assert!(ctx.estimated_tokens > 0);
    }

    #[test]
    fn test_assemble_with_patterns() {
        let assembler = ContextAssembler::new(4000);
        let patterns = vec![
            ("high cpu".to_string(), "restart service".to_string(), 0.9),
            ("disk full".to_string(), "cleanup tmp".to_string(), 0.8),
        ];
        let tools = vec!["process".to_string(), "fs".to_string()];

        let ctx = assembler
            .assemble_for_task("Handle high CPU usage", "operational", &patterns, &tools)
            .unwrap();

        assert_eq!(ctx.memory_context.len(), 2);
        assert_eq!(ctx.available_tools.len(), 2);
        assert!(ctx.system_prompt.contains("Handle high CPU usage"));
    }

    #[test]
    fn test_assemble_respects_token_limit() {
        let assembler = ContextAssembler::new(10); // Very small limit
        let patterns: Vec<_> = (0..100)
            .map(|i| (format!("trigger_{i}"), format!("action_{i}"), 0.5))
            .collect();

        let ctx = assembler
            .assemble_for_task("task", "reactive", &patterns, &[])
            .unwrap();

        // Should have limited the number of chunks
        assert!(ctx.memory_context.len() < 100);
    }

    #[test]
    fn test_build_system_prompt() {
        let prompt = build_system_prompt("restart nginx", "operational", &["service".to_string()]);
        assert!(prompt.contains("restart nginx"));
        assert!(prompt.contains("operational"));
        assert!(prompt.contains("service"));
    }

    #[test]
    fn test_estimate_tokens() {
        // 20 chars should be ~5 tokens
        assert_eq!(estimate_tokens("12345678901234567890"), 5);
        assert_eq!(estimate_tokens(""), 0);
    }
}
