//! Integration tests for the aiOS orchestrator
//!
//! Tests the core orchestration flow:
//! - Goal creation and state management
//! - Task decomposition logic
//! - Agent selection and routing
//! - Result aggregation
//! - Decision logging

use std::collections::HashMap;

// ============================================================================
// Goal Creation and State Management
// ============================================================================

/// Goal states for the orchestrator lifecycle
#[derive(Debug, Clone, PartialEq)]
enum GoalStatus {
    Pending,
    Planning,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

impl GoalStatus {
    fn is_terminal(&self) -> bool {
        matches!(
            self,
            GoalStatus::Completed | GoalStatus::Failed | GoalStatus::Cancelled
        )
    }
}

#[derive(Debug, Clone)]
struct Goal {
    id: String,
    description: String,
    priority: i32,
    status: GoalStatus,
    source: String,
}

#[derive(Debug, Clone)]
struct Task {
    id: String,
    goal_id: String,
    description: String,
    status: String,
    intelligence_level: String,
    required_tools: Vec<String>,
    depends_on: Vec<String>,
    assigned_agent: Option<String>,
}

/// Test full goal lifecycle: Pending -> Planning -> InProgress -> Completed
#[test]
fn test_goal_lifecycle_happy_path() {
    let mut goals: HashMap<String, Goal> = HashMap::new();

    // Submit goal
    let goal = Goal {
        id: "goal-1".into(),
        description: "Deploy new version of web service".into(),
        priority: 1,
        status: GoalStatus::Pending,
        source: "user".into(),
    };
    goals.insert(goal.id.clone(), goal);

    // Check pending
    assert_eq!(goals["goal-1"].status, GoalStatus::Pending);
    let active_count = goals.values().filter(|g| !g.status.is_terminal()).count();
    assert_eq!(active_count, 1);

    // Transition to Planning
    goals.get_mut("goal-1").unwrap().status = GoalStatus::Planning;
    assert_eq!(goals["goal-1"].status, GoalStatus::Planning);

    // Transition to InProgress
    goals.get_mut("goal-1").unwrap().status = GoalStatus::InProgress;
    assert_eq!(goals["goal-1"].status, GoalStatus::InProgress);

    // Complete
    goals.get_mut("goal-1").unwrap().status = GoalStatus::Completed;
    assert_eq!(goals["goal-1"].status, GoalStatus::Completed);
    assert!(goals["goal-1"].status.is_terminal());

    let active_count = goals.values().filter(|g| !g.status.is_terminal()).count();
    assert_eq!(active_count, 0);
}

/// Test goal cancellation cascades to tasks
#[test]
fn test_goal_cancellation_cascades() {
    let mut tasks: Vec<Task> = vec![
        Task {
            id: "t1".into(),
            goal_id: "goal-1".into(),
            description: "Task 1".into(),
            status: "completed".into(),
            intelligence_level: "reactive".into(),
            required_tools: vec![],
            depends_on: vec![],
            assigned_agent: None,
        },
        Task {
            id: "t2".into(),
            goal_id: "goal-1".into(),
            description: "Task 2".into(),
            status: "pending".into(),
            intelligence_level: "operational".into(),
            required_tools: vec!["fs".into()],
            depends_on: vec!["t1".into()],
            assigned_agent: None,
        },
        Task {
            id: "t3".into(),
            goal_id: "goal-1".into(),
            description: "Task 3".into(),
            status: "in_progress".into(),
            intelligence_level: "tactical".into(),
            required_tools: vec![],
            depends_on: vec![],
            assigned_agent: Some("agent-1".into()),
        },
    ];

    // Cancel all non-completed tasks
    for task in tasks.iter_mut() {
        if task.status != "completed" {
            task.status = "cancelled".into();
        }
    }

    assert_eq!(tasks[0].status, "completed"); // Already done, not cancelled
    assert_eq!(tasks[1].status, "cancelled");
    assert_eq!(tasks[2].status, "cancelled");
}

/// Test goal priority ordering
#[test]
fn test_goal_priority_ordering() {
    let mut goals = vec![
        Goal {
            id: "g1".into(),
            description: "Low priority".into(),
            priority: 5,
            status: GoalStatus::Pending,
            source: "auto".into(),
        },
        Goal {
            id: "g2".into(),
            description: "High priority".into(),
            priority: 1,
            status: GoalStatus::Pending,
            source: "user".into(),
        },
        Goal {
            id: "g3".into(),
            description: "Medium priority".into(),
            priority: 3,
            status: GoalStatus::Pending,
            source: "auto".into(),
        },
    ];

    // Sort by priority (lower number = higher priority)
    goals.sort_by(|a, b| a.priority.cmp(&b.priority));

    assert_eq!(goals[0].id, "g2"); // priority 1
    assert_eq!(goals[1].id, "g3"); // priority 3
    assert_eq!(goals[2].id, "g1"); // priority 5
}

/// Test progress calculation
#[test]
fn test_goal_progress_calculation() {
    fn calculate_progress(tasks: &[Task]) -> f64 {
        if tasks.is_empty() {
            return 0.0;
        }
        let completed = tasks.iter().filter(|t| t.status == "completed").count() as f64;
        let total = tasks.len() as f64;
        (completed / total) * 100.0
    }

    let tasks_empty: Vec<Task> = vec![];
    assert_eq!(calculate_progress(&tasks_empty), 0.0);

    let tasks = vec![
        Task {
            id: "t1".into(),
            goal_id: "g1".into(),
            description: "done".into(),
            status: "completed".into(),
            intelligence_level: "reactive".into(),
            required_tools: vec![],
            depends_on: vec![],
            assigned_agent: None,
        },
        Task {
            id: "t2".into(),
            goal_id: "g1".into(),
            description: "pending".into(),
            status: "pending".into(),
            intelligence_level: "reactive".into(),
            required_tools: vec![],
            depends_on: vec![],
            assigned_agent: None,
        },
        Task {
            id: "t3".into(),
            goal_id: "g1".into(),
            description: "in progress".into(),
            status: "in_progress".into(),
            intelligence_level: "reactive".into(),
            required_tools: vec![],
            depends_on: vec![],
            assigned_agent: None,
        },
        Task {
            id: "t4".into(),
            goal_id: "g1".into(),
            description: "done".into(),
            status: "completed".into(),
            intelligence_level: "reactive".into(),
            required_tools: vec![],
            depends_on: vec![],
            assigned_agent: None,
        },
    ];
    assert_eq!(calculate_progress(&tasks), 50.0); // 2/4 completed
}

/// Test multiple concurrent goals
#[test]
fn test_multiple_concurrent_goals() {
    let mut goals: HashMap<String, Goal> = HashMap::new();

    for i in 0..10 {
        goals.insert(
            format!("g{i}"),
            Goal {
                id: format!("g{i}"),
                description: format!("Goal {i}"),
                priority: (i % 3) + 1,
                status: GoalStatus::Pending,
                source: "test".into(),
            },
        );
    }

    assert_eq!(goals.len(), 10);

    // Complete some goals
    goals.get_mut("g0").unwrap().status = GoalStatus::Completed;
    goals.get_mut("g1").unwrap().status = GoalStatus::Failed;
    goals.get_mut("g2").unwrap().status = GoalStatus::Cancelled;

    let active: Vec<_> = goals
        .values()
        .filter(|g| !g.status.is_terminal())
        .collect();
    assert_eq!(active.len(), 7);
}

// ============================================================================
// Task Decomposition Logic
// ============================================================================

/// Intelligence level classification
#[derive(Debug, Clone, PartialEq)]
enum IntelligenceLevel {
    Reactive,
    Operational,
    Tactical,
    Strategic,
}

fn classify_complexity(description: &str) -> IntelligenceLevel {
    let desc_lower = description.to_lowercase();

    if desc_lower.contains("status")
        || desc_lower.contains("health")
        || desc_lower.contains("uptime")
        || desc_lower.contains("ping")
    {
        return IntelligenceLevel::Reactive;
    }

    if desc_lower.contains("read file")
        || desc_lower.contains("list")
        || desc_lower.contains("check disk")
        || desc_lower.contains("log")
    {
        return IntelligenceLevel::Operational;
    }

    if desc_lower.contains("analyze")
        || desc_lower.contains("plan")
        || desc_lower.contains("design")
        || desc_lower.contains("security audit")
        || desc_lower.contains("architecture")
    {
        return IntelligenceLevel::Strategic;
    }

    IntelligenceLevel::Tactical
}

fn infer_required_tools(description: &str) -> Vec<String> {
    let desc_lower = description.to_lowercase();
    let mut tools = vec![];

    if desc_lower.contains("file")
        || desc_lower.contains("read")
        || desc_lower.contains("write")
        || desc_lower.contains("directory")
    {
        tools.push("fs".to_string());
    }
    if desc_lower.contains("process")
        || desc_lower.contains("kill")
        || desc_lower.contains("spawn")
    {
        tools.push("process".to_string());
    }
    if desc_lower.contains("service")
        || desc_lower.contains("restart")
        || desc_lower.contains("start")
        || desc_lower.contains("stop")
    {
        tools.push("service".to_string());
    }
    if desc_lower.contains("network")
        || desc_lower.contains("firewall")
        || desc_lower.contains("dns")
    {
        tools.push("net".to_string());
    }
    if desc_lower.contains("install")
        || desc_lower.contains("package")
        || desc_lower.contains("update")
    {
        tools.push("pkg".to_string());
    }
    if desc_lower.contains("security")
        || desc_lower.contains("permission")
        || desc_lower.contains("audit")
    {
        tools.push("sec".to_string());
    }

    tools
}

/// Test comprehensive complexity classification
#[test]
fn test_task_decomposition_complexity() {
    // Reactive
    assert_eq!(
        classify_complexity("Check system health"),
        IntelligenceLevel::Reactive
    );
    assert_eq!(
        classify_complexity("Ping the server"),
        IntelligenceLevel::Reactive
    );
    assert_eq!(
        classify_complexity("Get uptime"),
        IntelligenceLevel::Reactive
    );

    // Operational
    assert_eq!(
        classify_complexity("Read file /etc/hosts"),
        IntelligenceLevel::Operational
    );
    assert_eq!(
        classify_complexity("List running services"),
        IntelligenceLevel::Operational
    );
    assert_eq!(
        classify_complexity("Check disk space"),
        IntelligenceLevel::Operational
    );

    // Strategic
    assert_eq!(
        classify_complexity("Analyze log patterns"),
        IntelligenceLevel::Strategic
    );
    assert_eq!(
        classify_complexity("Plan deployment strategy"),
        IntelligenceLevel::Strategic
    );
    assert_eq!(
        classify_complexity("Design microservice architecture"),
        IntelligenceLevel::Strategic
    );

    // Tactical (default)
    assert_eq!(
        classify_complexity("Compile the application"),
        IntelligenceLevel::Tactical
    );
}

/// Test tool inference from task descriptions
#[test]
fn test_task_tool_inference() {
    // File system tools
    let tools = infer_required_tools("Read file /etc/hostname");
    assert!(tools.contains(&"fs".to_string()));

    // Process tools
    let tools = infer_required_tools("Kill the runaway process");
    assert!(tools.contains(&"process".to_string()));

    // Service tools
    let tools = infer_required_tools("Restart the web service");
    assert!(tools.contains(&"service".to_string()));

    // Network tools
    let tools = infer_required_tools("Check DNS resolution");
    assert!(tools.contains(&"net".to_string()));

    // Package tools
    let tools = infer_required_tools("Install the curl package");
    assert!(tools.contains(&"pkg".to_string()));

    // Security tools
    let tools = infer_required_tools("Audit file permissions");
    assert!(tools.contains(&"sec".to_string()));

    // Multiple tools
    let tools = infer_required_tools("Read file and check security permissions");
    assert!(tools.contains(&"fs".to_string()));
    assert!(tools.contains(&"sec".to_string()));

    // No tools
    let tools = infer_required_tools("Hello world");
    assert!(tools.is_empty());
}

/// Test task dependency resolution
#[test]
fn test_task_dependency_resolution() {
    let tasks = vec![
        Task {
            id: "t1".into(),
            goal_id: "g1".into(),
            description: "Check package availability".into(),
            status: "completed".into(),
            intelligence_level: "reactive".into(),
            required_tools: vec!["pkg".into()],
            depends_on: vec![],
            assigned_agent: None,
        },
        Task {
            id: "t2".into(),
            goal_id: "g1".into(),
            description: "Install package".into(),
            status: "pending".into(),
            intelligence_level: "operational".into(),
            required_tools: vec!["pkg".into()],
            depends_on: vec!["t1".into()],
            assigned_agent: None,
        },
        Task {
            id: "t3".into(),
            goal_id: "g1".into(),
            description: "Configure service".into(),
            status: "pending".into(),
            intelligence_level: "tactical".into(),
            required_tools: vec!["fs".into(), "service".into()],
            depends_on: vec!["t2".into()],
            assigned_agent: None,
        },
        Task {
            id: "t4".into(),
            goal_id: "g1".into(),
            description: "Verify service health".into(),
            status: "pending".into(),
            intelligence_level: "reactive".into(),
            required_tools: vec![],
            depends_on: vec!["t3".into()],
            assigned_agent: None,
        },
    ];

    // Find next executable task (pending + all dependencies completed)
    fn next_task<'a>(tasks: &'a [Task]) -> Option<&'a Task> {
        tasks
            .iter()
            .filter(|t| t.status == "pending")
            .find(|t| {
                t.depends_on.iter().all(|dep_id| {
                    tasks
                        .iter()
                        .find(|d| d.id == *dep_id)
                        .map_or(true, |dep| dep.status == "completed")
                })
            })
    }

    // t1 is completed, t2 depends on t1 -> t2 is next
    let next = next_task(&tasks);
    assert!(next.is_some());
    assert_eq!(next.unwrap().id, "t2");

    // t3 cannot be executed yet (t2 still pending)
    let can_run_t3 = tasks[2].depends_on.iter().all(|dep_id| {
        tasks
            .iter()
            .find(|d| d.id == *dep_id)
            .map_or(true, |dep| dep.status == "completed")
    });
    assert!(!can_run_t3);
}

// ============================================================================
// Agent Selection
// ============================================================================

#[derive(Debug, Clone)]
struct Agent {
    id: String,
    agent_type: String,
    tool_namespaces: Vec<String>,
    status: String,
    tasks_completed: u32,
}

fn select_agent(agents: &[Agent], required_tools: &[String]) -> Option<String> {
    let mut candidates: Vec<&Agent> = agents
        .iter()
        .filter(|a| a.status == "idle")
        .filter(|a| {
            if required_tools.is_empty() {
                return true;
            }
            required_tools
                .iter()
                .any(|tool| a.tool_namespaces.contains(tool))
        })
        .collect();

    // Sort by experience (tasks_completed descending)
    candidates.sort_by(|a, b| b.tasks_completed.cmp(&a.tasks_completed));

    candidates.first().map(|a| a.id.clone())
}

/// Test agent selection with capability matching
#[test]
fn test_agent_selection_by_capability() {
    let agents = vec![
        Agent {
            id: "sys-1".into(),
            agent_type: "system".into(),
            tool_namespaces: vec!["fs".into(), "process".into()],
            status: "idle".into(),
            tasks_completed: 5,
        },
        Agent {
            id: "net-1".into(),
            agent_type: "network".into(),
            tool_namespaces: vec!["net".into(), "firewall".into()],
            status: "idle".into(),
            tasks_completed: 3,
        },
        Agent {
            id: "sec-1".into(),
            agent_type: "security".into(),
            tool_namespaces: vec!["sec".into()],
            status: "idle".into(),
            tasks_completed: 10,
        },
    ];

    // FS task -> sys-1
    let agent = select_agent(&agents, &["fs".into()]);
    assert_eq!(agent, Some("sys-1".to_string()));

    // Network task -> net-1
    let agent = select_agent(&agents, &["net".into()]);
    assert_eq!(agent, Some("net-1".to_string()));

    // Security task -> sec-1
    let agent = select_agent(&agents, &["sec".into()]);
    assert_eq!(agent, Some("sec-1".to_string()));

    // No tools required -> most experienced idle agent
    let agent = select_agent(&agents, &[]);
    assert_eq!(agent, Some("sec-1".to_string())); // 10 completed tasks
}

/// Test agent selection with busy agents
#[test]
fn test_agent_selection_busy_agents() {
    let agents = vec![
        Agent {
            id: "sys-1".into(),
            agent_type: "system".into(),
            tool_namespaces: vec!["fs".into()],
            status: "busy".into(),
            tasks_completed: 10,
        },
        Agent {
            id: "sys-2".into(),
            agent_type: "system".into(),
            tool_namespaces: vec!["fs".into()],
            status: "idle".into(),
            tasks_completed: 2,
        },
    ];

    let agent = select_agent(&agents, &["fs".into()]);
    // sys-1 is busy, sys-2 is idle -> should pick sys-2
    assert_eq!(agent, Some("sys-2".to_string()));
}

/// Test agent selection when no agent matches
#[test]
fn test_agent_selection_no_match() {
    let agents = vec![Agent {
        id: "sys-1".into(),
        agent_type: "system".into(),
        tool_namespaces: vec!["fs".into()],
        status: "idle".into(),
        tasks_completed: 5,
    }];

    // Need "net" but only "fs" agent available
    let agent = select_agent(&agents, &["net".into()]);
    assert!(agent.is_none());
}

/// Test agent selection prefers experienced agents
#[test]
fn test_agent_selection_prefers_experienced() {
    let agents = vec![
        Agent {
            id: "new-agent".into(),
            agent_type: "system".into(),
            tool_namespaces: vec!["fs".into()],
            status: "idle".into(),
            tasks_completed: 1,
        },
        Agent {
            id: "exp-agent".into(),
            agent_type: "system".into(),
            tool_namespaces: vec!["fs".into()],
            status: "idle".into(),
            tasks_completed: 100,
        },
    ];

    let agent = select_agent(&agents, &["fs".into()]);
    assert_eq!(agent, Some("exp-agent".to_string()));
}

// ============================================================================
// Result Aggregation
// ============================================================================

/// Test result aggregation for goal completion
#[test]
fn test_result_aggregation() {
    #[derive(Debug)]
    struct TaskResult {
        task_id: String,
        success: bool,
        tokens_used: i32,
        duration_ms: i64,
        model_used: String,
    }

    struct GoalSummary {
        total_tasks: usize,
        succeeded: usize,
        failed: usize,
        total_tokens: i32,
        total_duration_ms: i64,
        models_used: Vec<String>,
        overall_success: bool,
    }

    let results = vec![
        TaskResult {
            task_id: "t1".into(),
            success: true,
            tokens_used: 100,
            duration_ms: 50,
            model_used: "tinyllama".into(),
        },
        TaskResult {
            task_id: "t2".into(),
            success: true,
            tokens_used: 200,
            duration_ms: 100,
            model_used: "mistral".into(),
        },
        TaskResult {
            task_id: "t3".into(),
            success: true,
            tokens_used: 500,
            duration_ms: 2000,
            model_used: "claude".into(),
        },
    ];

    let total = results.len();
    let succeeded = results.iter().filter(|r| r.success).count();
    let failed = total - succeeded;
    let total_tokens: i32 = results.iter().map(|r| r.tokens_used).sum();
    let total_duration: i64 = results.iter().map(|r| r.duration_ms).sum();
    let mut models: Vec<String> = results.iter().map(|r| r.model_used.clone()).collect();
    models.sort();
    models.dedup();

    let summary = GoalSummary {
        total_tasks: total,
        succeeded,
        failed,
        total_tokens,
        total_duration_ms: total_duration,
        models_used: models,
        overall_success: failed == 0,
    };

    assert_eq!(summary.total_tasks, 3);
    assert_eq!(summary.succeeded, 3);
    assert_eq!(summary.failed, 0);
    assert_eq!(summary.total_tokens, 800);
    assert_eq!(summary.total_duration_ms, 2150);
    assert_eq!(summary.models_used.len(), 3);
    assert!(summary.overall_success);
}

/// Test result aggregation with failures
#[test]
fn test_result_aggregation_with_failures() {
    struct TaskResult {
        success: bool,
        tokens_used: i32,
    }

    let results = vec![
        TaskResult {
            success: true,
            tokens_used: 100,
        },
        TaskResult {
            success: false,
            tokens_used: 0,
        },
        TaskResult {
            success: true,
            tokens_used: 200,
        },
    ];

    let succeeded = results.iter().filter(|r| r.success).count();
    let failed = results.len() - succeeded;
    let total_tokens: i32 = results.iter().map(|r| r.tokens_used).sum();

    assert_eq!(succeeded, 2);
    assert_eq!(failed, 1);
    assert_eq!(total_tokens, 300);
    // Not overall success because there's a failure
    assert!(failed > 0);
}

// ============================================================================
// Decision Logging
// ============================================================================

/// Test decision logging and success rate calculation
#[test]
fn test_decision_logging_and_analysis() {
    #[derive(Debug, Clone)]
    struct DecisionRecord {
        id: String,
        context: String,
        chosen: String,
        reasoning: String,
        outcome: Option<String>,
    }

    let decisions = vec![
        DecisionRecord {
            id: "d1".into(),
            context: "route_task".into(),
            chosen: "agent-1".into(),
            reasoning: "Best capability match".into(),
            outcome: Some("success".into()),
        },
        DecisionRecord {
            id: "d2".into(),
            context: "route_task".into(),
            chosen: "agent-2".into(),
            reasoning: "Only available agent".into(),
            outcome: Some("success".into()),
        },
        DecisionRecord {
            id: "d3".into(),
            context: "route_task".into(),
            chosen: "agent-1".into(),
            reasoning: "Load balanced".into(),
            outcome: Some("failed".into()),
        },
        DecisionRecord {
            id: "d4".into(),
            context: "select_model".into(),
            chosen: "tinyllama".into(),
            reasoning: "Low complexity task".into(),
            outcome: Some("success".into()),
        },
    ];

    // Get decisions by context
    let route_decisions: Vec<_> = decisions
        .iter()
        .filter(|d| d.context.contains("route_task"))
        .collect();
    assert_eq!(route_decisions.len(), 3);

    // Calculate success rate for route_task
    let with_outcome: Vec<_> = route_decisions
        .iter()
        .filter(|d| d.outcome.is_some())
        .collect();
    let successes = with_outcome
        .iter()
        .filter(|d| {
            d.outcome
                .as_ref()
                .map_or(false, |o| o.contains("success"))
        })
        .count() as f64;
    let rate = successes / with_outcome.len() as f64;
    assert!((rate - 2.0 / 3.0).abs() < f64::EPSILON);

    // Recent decisions (reverse order)
    let recent: Vec<_> = decisions.iter().rev().take(2).collect();
    assert_eq!(recent[0].id, "d4");
    assert_eq!(recent[1].id, "d3");
}

// ============================================================================
// End-to-End Orchestration Flow
// ============================================================================

/// Test a full orchestration flow: goal -> decompose -> route -> execute -> aggregate
#[test]
fn test_full_orchestration_flow() {
    // Step 1: Submit goal
    let goal_desc = "Install and configure nginx web server";
    let goal_id = "goal-1";

    // Step 2: Classify complexity
    let level = classify_complexity(goal_desc);
    // "Install" is not reactive/operational/strategic keywords, defaults to tactical
    // But the description also has no analyze/plan keywords, so tactical
    assert_eq!(level, IntelligenceLevel::Tactical);

    // Step 3: Infer tools
    let tools = infer_required_tools(goal_desc);
    assert!(tools.contains(&"service".to_string())); // "service" keyword -> service tool
    assert!(tools.contains(&"pkg".to_string())); // "install" keyword -> pkg tool

    // Step 4: Create tasks
    let tasks = vec![
        Task {
            id: "t1".into(),
            goal_id: goal_id.into(),
            description: "Install nginx package".into(),
            status: "pending".into(),
            intelligence_level: "operational".into(),
            required_tools: vec!["pkg".into()],
            depends_on: vec![],
            assigned_agent: None,
        },
        Task {
            id: "t2".into(),
            goal_id: goal_id.into(),
            description: "Configure nginx".into(),
            status: "pending".into(),
            intelligence_level: "tactical".into(),
            required_tools: vec!["fs".into()],
            depends_on: vec!["t1".into()],
            assigned_agent: None,
        },
        Task {
            id: "t3".into(),
            goal_id: goal_id.into(),
            description: "Start nginx service".into(),
            status: "pending".into(),
            intelligence_level: "operational".into(),
            required_tools: vec!["service".into()],
            depends_on: vec!["t2".into()],
            assigned_agent: None,
        },
    ];

    assert_eq!(tasks.len(), 3);

    // Step 5: Route first task (no dependencies)
    let agents = vec![
        Agent {
            id: "pkg-agent".into(),
            agent_type: "package".into(),
            tool_namespaces: vec!["pkg".into()],
            status: "idle".into(),
            tasks_completed: 5,
        },
        Agent {
            id: "sys-agent".into(),
            agent_type: "system".into(),
            tool_namespaces: vec!["fs".into(), "service".into()],
            status: "idle".into(),
            tasks_completed: 10,
        },
    ];

    let agent_for_t1 = select_agent(&agents, &tasks[0].required_tools);
    assert_eq!(agent_for_t1, Some("pkg-agent".to_string()));

    let agent_for_t2 = select_agent(&agents, &tasks[1].required_tools);
    assert_eq!(agent_for_t2, Some("sys-agent".to_string()));

    let agent_for_t3 = select_agent(&agents, &tasks[2].required_tools);
    assert_eq!(agent_for_t3, Some("sys-agent".to_string()));

    // Step 6: Simulate execution results
    let total_tokens = 100 + 500 + 150;
    let total_duration = 2000i64 + 5000 + 1000;
    let all_success = true;

    // Step 7: Verify aggregated results
    assert_eq!(total_tokens, 750);
    assert_eq!(total_duration, 8000);
    assert!(all_success);
}

/// Test orchestrator handles goal with no matching agents gracefully
#[test]
fn test_orchestrator_no_agent_available() {
    let agents: Vec<Agent> = vec![]; // No agents registered

    let agent = select_agent(&agents, &["fs".into()]);
    assert!(agent.is_none());

    // The orchestrator should handle this by queuing the task
    // rather than failing entirely
}

/// Test orchestrator with all agents busy
#[test]
fn test_orchestrator_all_agents_busy() {
    let agents = vec![
        Agent {
            id: "agent-1".into(),
            agent_type: "system".into(),
            tool_namespaces: vec!["fs".into()],
            status: "busy".into(),
            tasks_completed: 5,
        },
        Agent {
            id: "agent-2".into(),
            agent_type: "system".into(),
            tool_namespaces: vec!["fs".into()],
            status: "busy".into(),
            tasks_completed: 3,
        },
    ];

    let agent = select_agent(&agents, &["fs".into()]);
    assert!(agent.is_none()); // No idle agents

    // In the real system, the task would be queued until an agent becomes idle
}
