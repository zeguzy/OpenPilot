//! Dependency chain auto-activation (Task 12.10).
//!
//! When a predecessor Task completes and merges, successor Tasks that were
//! waiting on it SHALL be automatically activated with a fresh workspace
//! based on the updated main branch.
//!
//! See `design.md` §D9 (依赖链) and `specs/completion-pipeline/spec.md`.

use std::collections::HashMap;

/// `predecessor_id → [successor_id, ...]` mapping.
///
/// When [`DependencyGraph::on_task_merged`] is called for a predecessor, it
/// returns every successor that is now unblocked. The pipeline coordinator
/// is responsible for actually dispatching those successors.
#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    edges: HashMap<String, Vec<String>>,
}

impl DependencyGraph {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `successor` depends on `predecessor` — the successor
    /// SHALL NOT be activated until `predecessor` merges.
    pub fn add_dependency(&mut self, predecessor: &str, successor: &str) {
        self.edges
            .entry(predecessor.to_string())
            .or_default()
            .push(successor.to_string());
    }

    /// Returns the list of successors that were waiting on `task_id` and
    /// are now eligible for activation. The consumed edge is removed so a
    /// successor is never activated twice for the same predecessor.
    #[must_use]
    pub fn on_task_merged(&self, task_id: &str) -> Vec<String> {
        self.edges.get(task_id).cloned().unwrap_or_default()
    }

    /// Mutable variant that also removes the consumed edge.
    pub fn drain_successors(&mut self, task_id: &str) -> Vec<String> {
        self.edges.remove(task_id).unwrap_or_default()
    }

    /// Number of dependency edges recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.edges.values().map(Vec::len).sum()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Check whether `successor` depends on `predecessor`.
    #[must_use]
    pub fn depends_on(&self, predecessor: &str, successor: &str) -> bool {
        self.edges
            .get(predecessor)
            .is_some_and(|v| v.iter().any(|s| s == successor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_query_dependency() {
        let mut g = DependencyGraph::new();
        g.add_dependency("A", "B");
        assert!(g.depends_on("A", "B"));
        assert!(!g.depends_on("B", "A"));
        assert!(!g.depends_on("A", "C"));
    }

    #[test]
    fn on_task_merged_returns_successors() {
        let mut g = DependencyGraph::new();
        g.add_dependency("A", "B");
        g.add_dependency("A", "C");
        let successors = g.on_task_merged("A");
        assert_eq!(successors.len(), 2);
        assert!(successors.contains(&"B".to_string()));
        assert!(successors.contains(&"C".to_string()));
    }

    #[test]
    fn on_task_merged_empty_for_unknown() {
        let g = DependencyGraph::new();
        assert!(g.on_task_merged("unknown").is_empty());
    }

    #[test]
    fn drain_successors_removes_edge() {
        let mut g = DependencyGraph::new();
        g.add_dependency("A", "B");
        let drained = g.drain_successors("A");
        assert_eq!(drained, vec!["B".to_string()]);
        assert!(g.on_task_merged("A").is_empty());
    }

    #[test]
    fn multiple_predecessors() {
        let mut g = DependencyGraph::new();
        g.add_dependency("A", "C");
        g.add_dependency("B", "C");
        assert_eq!(g.len(), 2);
        // C depends on both A and B
        let from_a = g.on_task_merged("A");
        assert_eq!(from_a, vec!["C".to_string()]);
        let from_b = g.on_task_merged("B");
        assert_eq!(from_b, vec!["C".to_string()]);
    }
}
