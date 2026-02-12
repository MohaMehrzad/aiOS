//! Tool Registry — stores and retrieves tool definitions

use std::collections::HashMap;
use tracing::info;

use crate::proto::tools::ToolDefinition;

/// In-memory tool registry
pub struct Registry {
    tools: HashMap<String, ToolDefinition>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool definition
    pub fn register_tool(&mut self, tool: ToolDefinition) {
        info!("Registered tool: {} (ns: {})", tool.name, tool.namespace);
        self.tools.insert(tool.name.clone(), tool);
    }

    /// Get a tool by name
    pub fn get_tool(&self, name: &str) -> Option<ToolDefinition> {
        self.tools.get(name).cloned()
    }

    /// List tools, optionally filtered by namespace
    pub fn list_tools(&self, namespace: &str) -> Vec<ToolDefinition> {
        if namespace.is_empty() {
            self.tools.values().cloned().collect()
        } else {
            self.tools
                .values()
                .filter(|t| t.namespace == namespace)
                .cloned()
                .collect()
        }
    }

    /// Deregister a tool
    pub fn deregister_tool(&mut self, name: &str) {
        self.tools.remove(name);
    }

    /// Get total tool count
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tool(name: &str, namespace: &str) -> ToolDefinition {
        make_tool(name, namespace, "A test tool", vec![], "low", true, false, 5000)
    }

    #[test]
    fn test_register_and_get_tool() {
        let mut reg = Registry::new();
        reg.register_tool(sample_tool("fs.read", "fs"));

        let tool = reg.get_tool("fs.read");
        assert!(tool.is_some());
        let tool = tool.unwrap();
        assert_eq!(tool.name, "fs.read");
        assert_eq!(tool.namespace, "fs");
    }

    #[test]
    fn test_get_nonexistent_tool() {
        let reg = Registry::new();
        assert!(reg.get_tool("nonexistent").is_none());
    }

    #[test]
    fn test_list_tools_all() {
        let mut reg = Registry::new();
        reg.register_tool(sample_tool("fs.read", "fs"));
        reg.register_tool(sample_tool("fs.write", "fs"));
        reg.register_tool(sample_tool("net.ping", "net"));

        let all = reg.list_tools("");
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_list_tools_by_namespace() {
        let mut reg = Registry::new();
        reg.register_tool(sample_tool("fs.read", "fs"));
        reg.register_tool(sample_tool("fs.write", "fs"));
        reg.register_tool(sample_tool("net.ping", "net"));

        let fs_tools = reg.list_tools("fs");
        assert_eq!(fs_tools.len(), 2);

        let net_tools = reg.list_tools("net");
        assert_eq!(net_tools.len(), 1);

        let empty = reg.list_tools("nonexistent");
        assert_eq!(empty.len(), 0);
    }

    #[test]
    fn test_deregister_tool() {
        let mut reg = Registry::new();
        reg.register_tool(sample_tool("fs.read", "fs"));
        assert_eq!(reg.tool_count(), 1);

        reg.deregister_tool("fs.read");
        assert_eq!(reg.tool_count(), 0);
        assert!(reg.get_tool("fs.read").is_none());
    }

    #[test]
    fn test_deregister_nonexistent() {
        let mut reg = Registry::new();
        // Should not panic
        reg.deregister_tool("nonexistent");
    }

    #[test]
    fn test_tool_count() {
        let mut reg = Registry::new();
        assert_eq!(reg.tool_count(), 0);

        reg.register_tool(sample_tool("fs.read", "fs"));
        assert_eq!(reg.tool_count(), 1);

        reg.register_tool(sample_tool("fs.write", "fs"));
        assert_eq!(reg.tool_count(), 2);

        reg.deregister_tool("fs.read");
        assert_eq!(reg.tool_count(), 1);
    }

    #[test]
    fn test_register_overwrites_existing() {
        let mut reg = Registry::new();
        reg.register_tool(make_tool(
            "fs.read", "fs", "Original description", vec![], "low", true, false, 5000,
        ));

        reg.register_tool(make_tool(
            "fs.read", "fs", "Updated description", vec![], "medium", true, true, 10000,
        ));

        assert_eq!(reg.tool_count(), 1);
        let tool = reg.get_tool("fs.read").unwrap();
        assert_eq!(tool.description, "Updated description");
        assert_eq!(tool.risk_level, "medium");
        assert!(tool.reversible);
        assert_eq!(tool.timeout_ms, 10000);
    }

    #[test]
    fn test_make_tool_helper() {
        let tool = make_tool(
            "sec.audit",
            "sec",
            "Run security audit",
            vec!["root"],
            "critical",
            false,
            false,
            30000,
        );

        assert_eq!(tool.name, "sec.audit");
        assert_eq!(tool.namespace, "sec");
        assert_eq!(tool.version, "1.0.0");
        assert_eq!(tool.description, "Run security audit");
        assert_eq!(tool.required_capabilities, vec!["root".to_string()]);
        assert_eq!(tool.risk_level, "critical");
        assert!(tool.requires_confirmation); // critical -> requires_confirmation
        assert!(!tool.idempotent);
        assert!(!tool.reversible);
        assert_eq!(tool.timeout_ms, 30000);
        assert!(tool.rollback_tool.is_empty());
    }

    #[test]
    fn test_make_tool_non_critical() {
        let tool = make_tool("fs.read", "fs", "Read file", vec![], "low", true, false, 5000);
        assert!(!tool.requires_confirmation); // low -> no confirmation
    }

    #[test]
    fn test_register_many_tools() {
        let mut reg = Registry::new();
        for i in 0..100 {
            reg.register_tool(sample_tool(&format!("tool_{i}"), "batch"));
        }
        assert_eq!(reg.tool_count(), 100);

        let all = reg.list_tools("batch");
        assert_eq!(all.len(), 100);
    }

    #[test]
    fn test_list_tools_empty_registry() {
        let reg = Registry::new();
        let tools = reg.list_tools("");
        assert!(tools.is_empty());
    }

    #[test]
    fn test_register_multiple_namespaces() {
        let mut reg = Registry::new();
        let namespaces = vec!["fs", "net", "process", "service", "sec", "pkg"];
        for ns in &namespaces {
            reg.register_tool(sample_tool(&format!("{ns}.tool1"), ns));
            reg.register_tool(sample_tool(&format!("{ns}.tool2"), ns));
        }

        assert_eq!(reg.tool_count(), 12);

        for ns in &namespaces {
            let tools = reg.list_tools(ns);
            assert_eq!(tools.len(), 2, "Expected 2 tools in namespace {ns}");
        }
    }
}

/// Helper to create a ToolDefinition
pub fn make_tool(
    name: &str,
    namespace: &str,
    description: &str,
    required_capabilities: Vec<&str>,
    risk_level: &str,
    idempotent: bool,
    reversible: bool,
    timeout_ms: i32,
) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        namespace: namespace.to_string(),
        version: "1.0.0".to_string(),
        description: description.to_string(),
        input_schema: vec![],
        output_schema: vec![],
        required_capabilities: required_capabilities
            .into_iter()
            .map(|s| s.to_string())
            .collect(),
        risk_level: risk_level.to_string(),
        requires_confirmation: risk_level == "critical",
        idempotent,
        reversible,
        timeout_ms,
        rollback_tool: String::new(),
    }
}
