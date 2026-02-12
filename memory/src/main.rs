//! aiOS Memory Service — three-tier persistent memory
//!
//! Tiers:
//! - Operational: In-memory ring buffer for hot data (<1ms)
//! - Working: SQLite for warm data (<5ms)
//! - Long-term: SQLite + vector embeddings for cold data (<50ms)

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::transport::Server;
use tracing::info;

mod operational;
mod working;
mod longterm;
mod knowledge;
mod migration;

pub mod proto {
    pub mod common {
        tonic::include_proto!("aios.common");
    }
    pub mod memory {
        tonic::include_proto!("aios.memory");
    }
}

use proto::memory::memory_service_server::{MemoryService, MemoryServiceServer};

/// Shared memory state
pub struct MemoryState {
    pub operational: operational::OperationalMemory,
    pub working: working::WorkingMemory,
    pub longterm: longterm::LongTermMemory,
    pub knowledge: knowledge::KnowledgeBase,
}

/// gRPC service implementation
pub struct MemoryServiceImpl {
    state: Arc<RwLock<MemoryState>>,
}

#[tonic::async_trait]
impl MemoryService for MemoryServiceImpl {
    // --- Operational Memory ---

    async fn push_event(
        &self,
        request: tonic::Request<proto::memory::Event>,
    ) -> Result<tonic::Response<proto::memory::Empty>, tonic::Status> {
        let event = request.into_inner();
        let mut state = self.state.write().await;
        state.operational.push_event(event);
        Ok(tonic::Response::new(proto::memory::Empty {}))
    }

    async fn get_recent_events(
        &self,
        request: tonic::Request<proto::memory::RecentEventsRequest>,
    ) -> Result<tonic::Response<proto::memory::EventList>, tonic::Status> {
        let req = request.into_inner();
        let state = self.state.read().await;
        let events = state
            .operational
            .get_recent(req.count as usize, &req.category, &req.source);
        Ok(tonic::Response::new(proto::memory::EventList { events }))
    }

    async fn update_metric(
        &self,
        request: tonic::Request<proto::memory::MetricUpdate>,
    ) -> Result<tonic::Response<proto::memory::Empty>, tonic::Status> {
        let metric = request.into_inner();
        let mut state = self.state.write().await;
        state.operational.update_metric(metric);
        Ok(tonic::Response::new(proto::memory::Empty {}))
    }

    async fn get_metric(
        &self,
        request: tonic::Request<proto::memory::MetricRequest>,
    ) -> Result<tonic::Response<proto::memory::MetricValue>, tonic::Status> {
        let req = request.into_inner();
        let state = self.state.read().await;
        let value = state
            .operational
            .get_metric(&req.key)
            .ok_or_else(|| tonic::Status::not_found(format!("Metric not found: {}", req.key)))?;
        Ok(tonic::Response::new(value))
    }

    async fn get_system_snapshot(
        &self,
        _request: tonic::Request<proto::memory::Empty>,
    ) -> Result<tonic::Response<proto::memory::SystemSnapshot>, tonic::Status> {
        let state = self.state.read().await;
        let snapshot = state.operational.get_snapshot();
        Ok(tonic::Response::new(snapshot))
    }

    // --- Working Memory ---

    async fn store_goal(
        &self,
        request: tonic::Request<proto::memory::GoalRecord>,
    ) -> Result<tonic::Response<proto::memory::Empty>, tonic::Status> {
        let goal = request.into_inner();
        let state = self.state.read().await;
        state
            .working
            .store_goal(&goal)
            .map_err(|e| tonic::Status::internal(format!("Failed to store goal: {e}")))?;
        Ok(tonic::Response::new(proto::memory::Empty {}))
    }

    async fn update_goal(
        &self,
        request: tonic::Request<proto::memory::GoalUpdate>,
    ) -> Result<tonic::Response<proto::memory::Empty>, tonic::Status> {
        let update = request.into_inner();
        let state = self.state.read().await;
        state
            .working
            .update_goal(&update)
            .map_err(|e| tonic::Status::internal(format!("Failed to update goal: {e}")))?;
        Ok(tonic::Response::new(proto::memory::Empty {}))
    }

    async fn get_active_goals(
        &self,
        _request: tonic::Request<proto::memory::Empty>,
    ) -> Result<tonic::Response<proto::memory::GoalList>, tonic::Status> {
        let state = self.state.read().await;
        let goals = state
            .working
            .get_active_goals()
            .map_err(|e| tonic::Status::internal(format!("Failed to get goals: {e}")))?;
        Ok(tonic::Response::new(proto::memory::GoalList { goals }))
    }

    async fn store_task(
        &self,
        request: tonic::Request<proto::memory::TaskRecord>,
    ) -> Result<tonic::Response<proto::memory::Empty>, tonic::Status> {
        let task = request.into_inner();
        let state = self.state.read().await;
        state
            .working
            .store_task(&task)
            .map_err(|e| tonic::Status::internal(format!("Failed to store task: {e}")))?;
        Ok(tonic::Response::new(proto::memory::Empty {}))
    }

    async fn get_tasks_for_goal(
        &self,
        request: tonic::Request<proto::memory::GoalIdRequest>,
    ) -> Result<tonic::Response<proto::memory::TaskList>, tonic::Status> {
        let req = request.into_inner();
        let state = self.state.read().await;
        let tasks = state
            .working
            .get_tasks_for_goal(&req.goal_id)
            .map_err(|e| tonic::Status::internal(format!("Failed to get tasks: {e}")))?;
        Ok(tonic::Response::new(proto::memory::TaskList { tasks }))
    }

    async fn store_tool_call(
        &self,
        request: tonic::Request<proto::memory::ToolCallRecord>,
    ) -> Result<tonic::Response<proto::memory::Empty>, tonic::Status> {
        let record = request.into_inner();
        let state = self.state.read().await;
        state
            .working
            .store_tool_call(&record)
            .map_err(|e| tonic::Status::internal(format!("Failed to store tool call: {e}")))?;
        Ok(tonic::Response::new(proto::memory::Empty {}))
    }

    async fn store_decision(
        &self,
        request: tonic::Request<proto::memory::Decision>,
    ) -> Result<tonic::Response<proto::memory::Empty>, tonic::Status> {
        let decision = request.into_inner();
        let state = self.state.read().await;
        state
            .working
            .store_decision(&decision)
            .map_err(|e| tonic::Status::internal(format!("Failed to store decision: {e}")))?;
        Ok(tonic::Response::new(proto::memory::Empty {}))
    }

    async fn store_pattern(
        &self,
        request: tonic::Request<proto::memory::Pattern>,
    ) -> Result<tonic::Response<proto::memory::Empty>, tonic::Status> {
        let pattern = request.into_inner();
        let state = self.state.read().await;
        state
            .working
            .store_pattern(&pattern)
            .map_err(|e| tonic::Status::internal(format!("Failed to store pattern: {e}")))?;
        Ok(tonic::Response::new(proto::memory::Empty {}))
    }

    async fn find_pattern(
        &self,
        request: tonic::Request<proto::memory::PatternQuery>,
    ) -> Result<tonic::Response<proto::memory::PatternResult>, tonic::Status> {
        let query = request.into_inner();
        let state = self.state.read().await;
        let result = state
            .working
            .find_pattern(&query.trigger, query.min_success_rate)
            .map_err(|e| tonic::Status::internal(format!("Failed to find pattern: {e}")))?;
        Ok(tonic::Response::new(result))
    }

    async fn update_pattern_stats(
        &self,
        request: tonic::Request<proto::memory::PatternStatsUpdate>,
    ) -> Result<tonic::Response<proto::memory::Empty>, tonic::Status> {
        let update = request.into_inner();
        let state = self.state.read().await;
        state
            .working
            .update_pattern_stats(&update.id, update.success)
            .map_err(|e| tonic::Status::internal(format!("Failed to update pattern: {e}")))?;
        Ok(tonic::Response::new(proto::memory::Empty {}))
    }

    async fn store_agent_state(
        &self,
        request: tonic::Request<proto::memory::AgentState>,
    ) -> Result<tonic::Response<proto::memory::Empty>, tonic::Status> {
        let agent_state = request.into_inner();
        let state = self.state.read().await;
        state
            .working
            .store_agent_state(&agent_state)
            .map_err(|e| tonic::Status::internal(format!("Failed to store agent state: {e}")))?;
        Ok(tonic::Response::new(proto::memory::Empty {}))
    }

    async fn get_agent_state(
        &self,
        request: tonic::Request<proto::memory::AgentStateRequest>,
    ) -> Result<tonic::Response<proto::memory::AgentState>, tonic::Status> {
        let req = request.into_inner();
        let state = self.state.read().await;
        let agent_state = state
            .working
            .get_agent_state(&req.agent_name)
            .map_err(|e| tonic::Status::internal(format!("Failed to get agent state: {e}")))?;
        Ok(tonic::Response::new(agent_state))
    }

    // --- Long-Term Memory ---

    async fn semantic_search(
        &self,
        request: tonic::Request<proto::memory::SemanticSearchRequest>,
    ) -> Result<tonic::Response<proto::memory::SearchResults>, tonic::Status> {
        let req = request.into_inner();
        let state = self.state.read().await;
        let results = state
            .longterm
            .semantic_search(&req.query, &req.collections, req.n_results, req.min_relevance)
            .map_err(|e| tonic::Status::internal(format!("Semantic search failed: {e}")))?;
        Ok(tonic::Response::new(proto::memory::SearchResults {
            results,
        }))
    }

    async fn store_procedure(
        &self,
        request: tonic::Request<proto::memory::Procedure>,
    ) -> Result<tonic::Response<proto::memory::Empty>, tonic::Status> {
        let procedure = request.into_inner();
        let state = self.state.read().await;
        state
            .longterm
            .store_procedure(&procedure)
            .map_err(|e| tonic::Status::internal(format!("Failed to store procedure: {e}")))?;
        Ok(tonic::Response::new(proto::memory::Empty {}))
    }

    async fn store_incident(
        &self,
        request: tonic::Request<proto::memory::Incident>,
    ) -> Result<tonic::Response<proto::memory::Empty>, tonic::Status> {
        let incident = request.into_inner();
        let state = self.state.read().await;
        state
            .longterm
            .store_incident(&incident)
            .map_err(|e| tonic::Status::internal(format!("Failed to store incident: {e}")))?;
        Ok(tonic::Response::new(proto::memory::Empty {}))
    }

    async fn store_config_change(
        &self,
        request: tonic::Request<proto::memory::ConfigChange>,
    ) -> Result<tonic::Response<proto::memory::Empty>, tonic::Status> {
        let change = request.into_inner();
        let state = self.state.read().await;
        state
            .longterm
            .store_config_change(&change)
            .map_err(|e| tonic::Status::internal(format!("Failed to store config change: {e}")))?;
        Ok(tonic::Response::new(proto::memory::Empty {}))
    }

    // --- Knowledge Base ---

    async fn search_knowledge(
        &self,
        request: tonic::Request<proto::memory::SemanticSearchRequest>,
    ) -> Result<tonic::Response<proto::memory::SearchResults>, tonic::Status> {
        let req = request.into_inner();
        let state = self.state.read().await;
        let results = state
            .knowledge
            .search(&req.query, req.n_results)
            .map_err(|e| tonic::Status::internal(format!("Knowledge search failed: {e}")))?;
        Ok(tonic::Response::new(proto::memory::SearchResults {
            results,
        }))
    }

    async fn add_knowledge(
        &self,
        request: tonic::Request<proto::memory::KnowledgeEntry>,
    ) -> Result<tonic::Response<proto::memory::Empty>, tonic::Status> {
        let entry = request.into_inner();
        let mut state = self.state.write().await;
        state
            .knowledge
            .add_entry(&entry)
            .map_err(|e| tonic::Status::internal(format!("Failed to add knowledge: {e}")))?;
        Ok(tonic::Response::new(proto::memory::Empty {}))
    }

    // --- Context Assembly ---

    async fn assemble_context(
        &self,
        request: tonic::Request<proto::memory::ContextRequest>,
    ) -> Result<tonic::Response<proto::memory::ContextResponse>, tonic::Status> {
        let req = request.into_inner();
        let state = self.state.read().await;

        let mut chunks = Vec::new();
        let max_tokens = if req.max_tokens == 0 {
            4000
        } else {
            req.max_tokens
        };
        let mut total_tokens = 0i32;

        // Gather from each requested tier
        let tiers = if req.memory_tiers.is_empty() {
            vec![
                "operational".to_string(),
                "working".to_string(),
                "longterm".to_string(),
                "knowledge".to_string(),
            ]
        } else {
            req.memory_tiers
        };

        for tier in &tiers {
            if total_tokens >= max_tokens {
                break;
            }
            let _remaining = max_tokens - total_tokens;

            match tier.as_str() {
                "operational" => {
                    let events = state.operational.get_recent(10, "", "");
                    for event in events {
                        let content = String::from_utf8_lossy(&event.data_json).to_string();
                        let tokens = estimate_tokens(&content);
                        if total_tokens + tokens > max_tokens {
                            break;
                        }
                        chunks.push(proto::memory::ContextChunk {
                            source: "operational".into(),
                            content,
                            relevance: 0.8,
                            tokens,
                        });
                        total_tokens += tokens;
                    }
                }
                "working" => {
                    if let Ok(goals) = state.working.get_active_goals() {
                        for goal in goals.iter().take(5) {
                            let content = format!(
                                "Goal [{}]: {} (status: {})",
                                goal.id, goal.description, goal.status
                            );
                            let tokens = estimate_tokens(&content);
                            if total_tokens + tokens > max_tokens {
                                break;
                            }
                            chunks.push(proto::memory::ContextChunk {
                                source: "working".into(),
                                content,
                                relevance: 0.7,
                                tokens,
                            });
                            total_tokens += tokens;
                        }
                    }
                }
                "longterm" => {
                    if let Ok(results) = state.longterm.semantic_search(
                        &req.task_description,
                        &["decisions".into(), "procedures".into()],
                        5,
                        0.3,
                    ) {
                        for result in results {
                            let tokens = estimate_tokens(&result.content);
                            if total_tokens + tokens > max_tokens {
                                break;
                            }
                            chunks.push(proto::memory::ContextChunk {
                                source: "longterm".into(),
                                content: result.content,
                                relevance: result.relevance,
                                tokens,
                            });
                            total_tokens += tokens;
                        }
                    }
                }
                "knowledge" => {
                    if let Ok(results) = state.knowledge.search(&req.task_description, 5) {
                        for result in results {
                            let tokens = estimate_tokens(&result.content);
                            if total_tokens + tokens > max_tokens {
                                break;
                            }
                            chunks.push(proto::memory::ContextChunk {
                                source: "knowledge".into(),
                                content: result.content,
                                relevance: result.relevance,
                                tokens,
                            });
                            total_tokens += tokens;
                        }
                    }
                }
                _ => {}
            }
        }

        // Sort by relevance
        chunks.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap_or(std::cmp::Ordering::Equal));

        Ok(tonic::Response::new(proto::memory::ContextResponse {
            chunks,
            total_tokens,
        }))
    }
}

/// Rough token estimation (4 chars per token)
fn estimate_tokens(text: &str) -> i32 {
    (text.len() as f64 / 4.0).ceil() as i32
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .compact()
        .init();

    info!("aiOS Memory Service starting...");

    let working_db = std::env::var("AIOS_WORKING_DB")
        .unwrap_or_else(|_| "/var/lib/aios/memory/working.db".into());
    let longterm_db = std::env::var("AIOS_LONGTERM_DB")
        .unwrap_or_else(|_| "/var/lib/aios/memory/longterm.db".into());

    let state = Arc::new(RwLock::new(MemoryState {
        operational: operational::OperationalMemory::new(10000),
        working: working::WorkingMemory::new(&working_db)?,
        longterm: longterm::LongTermMemory::new(&longterm_db)?,
        knowledge: knowledge::KnowledgeBase::new()?,
    }));

    let service = MemoryServiceImpl { state };

    let addr: SocketAddr = "0.0.0.0:50053".parse()?;
    info!("Memory Service gRPC server listening on {addr}");

    Server::builder()
        .add_service(MemoryServiceServer::new(service))
        .serve(addr)
        .await
        .context("Memory Service gRPC server failed")?;

    Ok(())
}
