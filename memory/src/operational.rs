//! Operational Memory — in-memory ring buffer for hot data
//!
//! Sub-millisecond access, stores recent events and current metrics.

use std::collections::{HashMap, VecDeque};

use crate::proto::memory::{Event, MetricUpdate, MetricValue, SystemSnapshot};

/// In-memory ring buffer for operational data
pub struct OperationalMemory {
    events: VecDeque<Event>,
    metrics: HashMap<String, MetricValue>,
    max_entries: usize,
}

impl OperationalMemory {
    pub fn new(max_entries: usize) -> Self {
        Self {
            events: VecDeque::with_capacity(max_entries),
            metrics: HashMap::new(),
            max_entries,
        }
    }

    /// Push a new event into the ring buffer
    pub fn push_event(&mut self, event: Event) {
        if self.events.len() >= self.max_entries {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    /// Get recent events with optional filtering
    pub fn get_recent(&self, count: usize, category: &str, source: &str) -> Vec<Event> {
        self.events
            .iter()
            .rev()
            .filter(|e| category.is_empty() || e.category == category)
            .filter(|e| source.is_empty() || e.source == source)
            .take(count)
            .cloned()
            .collect()
    }

    /// Update a metric value
    pub fn update_metric(&mut self, update: MetricUpdate) {
        self.metrics.insert(
            update.key.clone(),
            MetricValue {
                key: update.key,
                value: update.value,
                timestamp: update.timestamp,
            },
        );
    }

    /// Get a metric value
    pub fn get_metric(&self, key: &str) -> Option<MetricValue> {
        self.metrics.get(key).cloned()
    }

    /// Get system snapshot from current metrics
    pub fn get_snapshot(&self) -> SystemSnapshot {
        SystemSnapshot {
            cpu_percent: self.metrics.get("cpu.usage").map_or(0.0, |m| m.value),
            memory_used_mb: self.metrics.get("memory.used_mb").map_or(0.0, |m| m.value),
            memory_total_mb: self.metrics.get("memory.total_mb").map_or(0.0, |m| m.value),
            disk_used_gb: self.metrics.get("disk.used_gb").map_or(0.0, |m| m.value),
            disk_total_gb: self.metrics.get("disk.total_gb").map_or(0.0, |m| m.value),
            gpu_utilization: self.metrics.get("gpu.utilization").map_or(0.0, |m| m.value),
            active_tasks: self
                .metrics
                .get("tasks.active")
                .map_or(0, |m| m.value as i32),
            active_agents: self
                .metrics
                .get("agents.active")
                .map_or(0, |m| m.value as i32),
            loaded_models: vec![],
        }
    }

    /// Get event count
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Get metric count
    pub fn metric_count(&self) -> usize {
        self.metrics.len()
    }

    /// Clear all events
    pub fn clear_events(&mut self) {
        self.events.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(id: &str, category: &str) -> Event {
        Event {
            id: id.to_string(),
            timestamp: 0,
            category: category.to_string(),
            source: "test".to_string(),
            data_json: b"{}".to_vec(),
            critical: false,
        }
    }

    #[test]
    fn test_ring_buffer() {
        let mut mem = OperationalMemory::new(3);
        mem.push_event(make_event("1", "metric"));
        mem.push_event(make_event("2", "event"));
        mem.push_event(make_event("3", "metric"));
        mem.push_event(make_event("4", "event")); // pushes out "1"

        assert_eq!(mem.event_count(), 3);
        let recent = mem.get_recent(10, "", "");
        assert_eq!(recent[0].id, "4");
    }

    #[test]
    fn test_filter_by_category() {
        let mut mem = OperationalMemory::new(100);
        mem.push_event(make_event("1", "metric"));
        mem.push_event(make_event("2", "event"));
        mem.push_event(make_event("3", "metric"));

        let metrics = mem.get_recent(10, "metric", "");
        assert_eq!(metrics.len(), 2);
    }

    #[test]
    fn test_metrics() {
        let mut mem = OperationalMemory::new(100);
        mem.update_metric(MetricUpdate {
            key: "cpu.usage".into(),
            value: 45.5,
            timestamp: 0,
        });

        let val = mem.get_metric("cpu.usage").unwrap();
        assert_eq!(val.value, 45.5);
    }

    #[test]
    fn test_ring_buffer_exact_capacity() {
        let mut mem = OperationalMemory::new(3);
        mem.push_event(make_event("1", "a"));
        mem.push_event(make_event("2", "a"));
        mem.push_event(make_event("3", "a"));

        assert_eq!(mem.event_count(), 3);
        // No overflow yet -- all events present
        let events = mem.get_recent(10, "", "");
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn test_ring_buffer_overflow_multiple() {
        let mut mem = OperationalMemory::new(2);
        mem.push_event(make_event("1", "a"));
        mem.push_event(make_event("2", "a"));
        mem.push_event(make_event("3", "a"));
        mem.push_event(make_event("4", "a"));
        mem.push_event(make_event("5", "a"));

        assert_eq!(mem.event_count(), 2);
        let events = mem.get_recent(10, "", "");
        assert_eq!(events[0].id, "5");
        assert_eq!(events[1].id, "4");
    }

    #[test]
    fn test_get_recent_limited_count() {
        let mut mem = OperationalMemory::new(100);
        for i in 0..10 {
            mem.push_event(make_event(&i.to_string(), "metric"));
        }

        let events = mem.get_recent(3, "", "");
        assert_eq!(events.len(), 3);
        // Newest first
        assert_eq!(events[0].id, "9");
        assert_eq!(events[1].id, "8");
        assert_eq!(events[2].id, "7");
    }

    #[test]
    fn test_filter_by_source() {
        let mut mem = OperationalMemory::new(100);
        let mut e1 = make_event("1", "metric");
        e1.source = "agent-1".to_string();
        let mut e2 = make_event("2", "metric");
        e2.source = "agent-2".to_string();
        let mut e3 = make_event("3", "event");
        e3.source = "agent-1".to_string();

        mem.push_event(e1);
        mem.push_event(e2);
        mem.push_event(e3);

        let agent1_events = mem.get_recent(10, "", "agent-1");
        assert_eq!(agent1_events.len(), 2);

        let agent2_events = mem.get_recent(10, "", "agent-2");
        assert_eq!(agent2_events.len(), 1);
    }

    #[test]
    fn test_filter_by_category_and_source() {
        let mut mem = OperationalMemory::new(100);
        let mut e1 = make_event("1", "metric");
        e1.source = "agent-1".to_string();
        let mut e2 = make_event("2", "event");
        e2.source = "agent-1".to_string();
        let mut e3 = make_event("3", "metric");
        e3.source = "agent-2".to_string();

        mem.push_event(e1);
        mem.push_event(e2);
        mem.push_event(e3);

        let filtered = mem.get_recent(10, "metric", "agent-1");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "1");
    }

    #[test]
    fn test_metric_update_overwrites() {
        let mut mem = OperationalMemory::new(100);
        mem.update_metric(MetricUpdate {
            key: "cpu.usage".into(),
            value: 10.0,
            timestamp: 100,
        });
        mem.update_metric(MetricUpdate {
            key: "cpu.usage".into(),
            value: 90.0,
            timestamp: 200,
        });

        let val = mem.get_metric("cpu.usage").unwrap();
        assert_eq!(val.value, 90.0);
        assert_eq!(val.timestamp, 200);
        assert_eq!(mem.metric_count(), 1);
    }

    #[test]
    fn test_get_metric_nonexistent() {
        let mem = OperationalMemory::new(100);
        assert!(mem.get_metric("nonexistent").is_none());
    }

    #[test]
    fn test_get_snapshot_defaults() {
        let mem = OperationalMemory::new(100);
        let snap = mem.get_snapshot();
        assert_eq!(snap.cpu_percent, 0.0);
        assert_eq!(snap.memory_used_mb, 0.0);
        assert_eq!(snap.memory_total_mb, 0.0);
        assert_eq!(snap.disk_used_gb, 0.0);
        assert_eq!(snap.disk_total_gb, 0.0);
        assert_eq!(snap.gpu_utilization, 0.0);
        assert_eq!(snap.active_tasks, 0);
        assert_eq!(snap.active_agents, 0);
    }

    #[test]
    fn test_get_snapshot_with_metrics() {
        let mut mem = OperationalMemory::new(100);
        mem.update_metric(MetricUpdate {
            key: "cpu.usage".into(),
            value: 75.0,
            timestamp: 0,
        });
        mem.update_metric(MetricUpdate {
            key: "memory.used_mb".into(),
            value: 8192.0,
            timestamp: 0,
        });
        mem.update_metric(MetricUpdate {
            key: "memory.total_mb".into(),
            value: 16384.0,
            timestamp: 0,
        });
        mem.update_metric(MetricUpdate {
            key: "tasks.active".into(),
            value: 5.0,
            timestamp: 0,
        });
        mem.update_metric(MetricUpdate {
            key: "agents.active".into(),
            value: 3.0,
            timestamp: 0,
        });

        let snap = mem.get_snapshot();
        assert_eq!(snap.cpu_percent, 75.0);
        assert_eq!(snap.memory_used_mb, 8192.0);
        assert_eq!(snap.memory_total_mb, 16384.0);
        assert_eq!(snap.active_tasks, 5);
        assert_eq!(snap.active_agents, 3);
    }

    #[test]
    fn test_clear_events() {
        let mut mem = OperationalMemory::new(100);
        mem.push_event(make_event("1", "a"));
        mem.push_event(make_event("2", "b"));
        assert_eq!(mem.event_count(), 2);

        mem.clear_events();
        assert_eq!(mem.event_count(), 0);

        let events = mem.get_recent(10, "", "");
        assert!(events.is_empty());
    }

    #[test]
    fn test_metric_count() {
        let mut mem = OperationalMemory::new(100);
        assert_eq!(mem.metric_count(), 0);

        mem.update_metric(MetricUpdate {
            key: "a".into(),
            value: 1.0,
            timestamp: 0,
        });
        mem.update_metric(MetricUpdate {
            key: "b".into(),
            value: 2.0,
            timestamp: 0,
        });
        assert_eq!(mem.metric_count(), 2);
    }

    #[test]
    fn test_event_with_critical_flag() {
        let mut mem = OperationalMemory::new(100);
        let mut event = make_event("1", "alert");
        event.critical = true;
        mem.push_event(event);

        let events = mem.get_recent(1, "", "");
        assert!(events[0].critical);
    }
}
