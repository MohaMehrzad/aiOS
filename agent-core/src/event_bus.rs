//! Event Bus — pub/sub system for system events
//!
//! Publishers emit events (e.g., from proactive.rs, health.rs, plugins).
//! Consumers subscribe with patterns and goal templates.
//! When events match subscriptions, goals are created automatically.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

/// A system event
#[derive(Debug, Clone)]
pub struct SystemEvent {
    pub id: String,
    pub event_type: String,
    pub source: String,
    pub data: serde_json::Value,
    pub timestamp: i64,
    pub severity: EventSeverity,
}

/// Event severity levels
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventSeverity {
    Info,
    Warning,
    Critical,
}

/// Subscription: event pattern -> goal template
#[derive(Debug, Clone)]
pub struct EventSubscription {
    pub id: String,
    pub event_pattern: String,
    pub goal_template: String,
    pub priority: i32,
    pub min_severity: EventSeverity,
}

/// The event bus
pub struct EventBus {
    subscriptions: HashMap<String, EventSubscription>,
    sender: mpsc::Sender<SystemEvent>,
    receiver: Option<mpsc::Receiver<SystemEvent>>,
    recent_events: Vec<SystemEvent>,
    max_recent: usize,
}

impl EventBus {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel(1000);
        Self {
            subscriptions: HashMap::new(),
            sender,
            receiver: Some(receiver),
            recent_events: Vec::new(),
            max_recent: 100,
        }
    }

    /// Get a sender handle for publishing events
    pub fn sender(&self) -> mpsc::Sender<SystemEvent> {
        self.sender.clone()
    }

    /// Subscribe to events matching a pattern
    pub fn subscribe(&mut self, subscription: EventSubscription) {
        info!(
            "Event subscription registered: {} -> {}",
            subscription.event_pattern,
            &subscription.goal_template[..60.min(subscription.goal_template.len())]
        );
        self.subscriptions
            .insert(subscription.id.clone(), subscription);
    }

    /// Unsubscribe
    pub fn unsubscribe(&mut self, subscription_id: &str) {
        self.subscriptions.remove(subscription_id);
    }

    /// List all subscriptions
    pub fn list_subscriptions(&self) -> Vec<&EventSubscription> {
        self.subscriptions.values().collect()
    }

    /// Get recent events
    pub fn recent_events(&self) -> &[SystemEvent] {
        &self.recent_events
    }

    /// Find subscriptions that match an event
    fn matching_subscriptions(&self, event: &SystemEvent) -> Vec<&EventSubscription> {
        self.subscriptions
            .values()
            .filter(|sub| {
                let pattern_match =
                    event.event_type.contains(&sub.event_pattern) || sub.event_pattern == "*";

                let severity_match = match (&sub.min_severity, &event.severity) {
                    (EventSeverity::Info, _) => true,
                    (EventSeverity::Warning, EventSeverity::Warning | EventSeverity::Critical) => {
                        true
                    }
                    (EventSeverity::Critical, EventSeverity::Critical) => true,
                    _ => false,
                };

                pattern_match && severity_match
            })
            .collect()
    }

    /// Run the event bus processing loop
    pub async fn run(
        bus: Arc<RwLock<Self>>,
        state: Arc<RwLock<crate::OrchestratorState>>,
        cancel: tokio_util::sync::CancellationToken,
    ) {
        let mut receiver = {
            let mut bus_w = bus.write().await;
            match bus_w.receiver.take() {
                Some(r) => r,
                None => {
                    warn!("Event bus receiver already taken");
                    return;
                }
            }
        };

        info!("Event bus started");

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("Event bus shutting down");
                    break;
                }
                event = receiver.recv() => {
                    match event {
                        Some(event) => {
                            let bus_r = bus.read().await;
                            let matches = bus_r.matching_subscriptions(&event);

                            if !matches.is_empty() {
                                let mut state_w = state.write().await;
                                for sub in matches {
                                    let goal_desc = sub.goal_template
                                        .replace("{event_type}", &event.event_type)
                                        .replace("{source}", &event.source);

                                    info!("Event {} triggered goal: {}", event.event_type,
                                        &goal_desc[..60.min(goal_desc.len())]);

                                    match state_w.goal_engine.submit_goal(
                                        goal_desc.clone(),
                                        sub.priority,
                                        format!("event_bus:{}", event.event_type),
                                    ).await {
                                        Ok(goal_id) => {
                                            if let Ok(tasks) = state_w.task_planner.decompose_goal(&goal_id, &goal_desc).await {
                                                state_w.goal_engine.add_tasks(&goal_id, tasks);
                                            }
                                        }
                                        Err(e) => {
                                            warn!("Failed to create event-triggered goal: {e}");
                                        }
                                    }
                                }
                            }

                            // Store in recent events
                            drop(bus_r);
                            let mut bus_w = bus.write().await;
                            bus_w.recent_events.push(event);
                            if bus_w.recent_events.len() > bus_w.max_recent {
                                bus_w.recent_events.remove(0);
                            }
                        }
                        None => break,
                    }
                }
            }
        }
    }
}

/// Helper to publish an event
pub async fn publish_event(
    sender: &mpsc::Sender<SystemEvent>,
    event_type: &str,
    source: &str,
    data: serde_json::Value,
    severity: EventSeverity,
) {
    let event = SystemEvent {
        id: uuid::Uuid::new_v4().to_string(),
        event_type: event_type.to_string(),
        source: source.to_string(),
        data,
        timestamp: chrono::Utc::now().timestamp(),
        severity,
    };

    if let Err(e) = sender.send(event).await {
        debug!("Failed to publish event: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_bus_new() {
        let bus = EventBus::new();
        assert!(bus.subscriptions.is_empty());
        assert!(bus.recent_events.is_empty());
    }

    #[test]
    fn test_subscribe() {
        let mut bus = EventBus::new();
        bus.subscribe(EventSubscription {
            id: "sub-1".to_string(),
            event_pattern: "cpu_high".to_string(),
            goal_template: "Investigate high CPU".to_string(),
            priority: 7,
            min_severity: EventSeverity::Warning,
        });
        assert_eq!(bus.list_subscriptions().len(), 1);
    }

    #[test]
    fn test_matching_subscriptions() {
        let mut bus = EventBus::new();
        bus.subscribe(EventSubscription {
            id: "sub-1".to_string(),
            event_pattern: "cpu".to_string(),
            goal_template: "Handle CPU issue".to_string(),
            priority: 7,
            min_severity: EventSeverity::Warning,
        });

        let event = SystemEvent {
            id: "ev-1".to_string(),
            event_type: "cpu_high".to_string(),
            source: "proactive".to_string(),
            data: serde_json::Value::Null,
            timestamp: 0,
            severity: EventSeverity::Critical,
        };

        let matches = bus.matching_subscriptions(&event);
        assert_eq!(matches.len(), 1);
    }
}
