// runtime/src/modules/graph.rs

use bua_core::{BuaError, BuaResult};
use dashmap::DashMap;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use super::record::{ModuleRecord, ModuleStatus};

/// Topological evaluation order for the module graph.
#[derive(Debug, Clone)]
pub struct EvalOrder(pub Vec<PathBuf>);

impl EvalOrder {
    pub fn iter(&self) -> impl Iterator<Item = &PathBuf> {
        self.0.iter()
    }
    pub fn len(&self) -> usize { self.0.len() }
    pub fn is_empty(&self) -> bool { self.0.is_empty() }
}

/// The module dependency graph for one agent execution.
///
/// Thread-safe reads; write operations go through &mut self (single writer
/// is the module loader, which runs on the JS thread).
pub struct ModuleGraph {
    /// All known modules, keyed by canonical resolved path string.
    nodes: HashMap<String, ModuleRecord>,
    /// Adjacency list: path -> [dependency paths]
    edges: HashMap<String, Vec<String>>,
}

impl ModuleGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
        }
    }

    /// Insert a module record into the graph.
    pub fn insert(&mut self, record: ModuleRecord) {
        let key = record.resolved_path.to_string_lossy().into_owned();
        self.edges.insert(key.clone(), Vec::new());
        self.nodes.insert(key, record);
    }

    /// Add a dependency edge: `from` imports `to`.
    pub fn add_edge(&mut self, from: &PathBuf, to: &PathBuf) {
        let from_key = from.to_string_lossy().into_owned();
        let to_key = to.to_string_lossy().into_owned();
        self.edges
            .entry(from_key)
            .or_default()
            .push(to_key);
    }

    pub fn get(&self, path: &PathBuf) -> Option<&ModuleRecord> {
        self.nodes.get(&path.to_string_lossy().into_owned())
    }

    pub fn get_mut(&mut self, path: &PathBuf) -> Option<&mut ModuleRecord> {
        self.nodes.get_mut(&path.to_string_lossy().into_owned())
    }

    pub fn contains(&self, path: &PathBuf) -> bool {
        self.nodes.contains_key(&path.to_string_lossy().into_owned())
    }

    pub fn len(&self) -> usize { self.nodes.len() }

    /// Compute a topological evaluation order starting from `entry`.
    ///
    /// Uses iterative post-order DFS with cycle detection.
    /// Cyclic imports are allowed (ESM semantics) but reported for diagnostics.
    /// Returns modules in dependency-first order (deepest dependencies first).
    pub fn eval_order(&self, entry: &PathBuf) -> BuaResult<EvalOrder> {
        let entry_key = entry.to_string_lossy().into_owned();

        if !self.nodes.contains_key(&entry_key) {
            return Err(BuaError::ModuleNotFound {
                specifier: entry_key,
            });
        }

        let mut visited: HashSet<String> = HashSet::new();
        let mut in_stack: HashSet<String> = HashSet::new();
        let mut order: Vec<PathBuf> = Vec::new();
        let mut cycles: Vec<String> = Vec::new();

        self.dfs_post_order(
            &entry_key,
            &mut visited,
            &mut in_stack,
            &mut order,
            &mut cycles,
        );

        if !cycles.is_empty() {
            tracing::warn!(
                cycles = cycles.join(" -> "),
                "ESM cyclic imports detected (allowed but may affect evaluation order)"
            );
        }

        Ok(EvalOrder(order))
    }

    fn dfs_post_order(
        &self,
        key: &str,
        visited: &mut HashSet<String>,
        in_stack: &mut HashSet<String>,
        order: &mut Vec<PathBuf>,
        cycles: &mut Vec<String>,
    ) {
        if in_stack.contains(key) {
            cycles.push(key.to_string());
            return; // Cycle — skip re-entry (module will be partially initialized per ESM spec)
        }
        if visited.contains(key) {
            return;
        }

        in_stack.insert(key.to_string());

        if let Some(deps) = self.edges.get(key) {
            for dep in deps.iter() {
                self.dfs_post_order(dep, visited, in_stack, order, cycles);
            }
        }

        in_stack.remove(key);
        visited.insert(key.to_string());

        if let Some(record) = self.nodes.get(key) {
            order.push(record.resolved_path.clone());
        }
    }

    /// Return all modules that have failed.
    pub fn failed_modules(&self) -> Vec<(&str, &str)> {
        self.nodes
            .iter()
            .filter_map(|(k, v)| {
                v.status.failed_reason().map(|r| (k.as_str(), r))
            })
            .collect()
    }

    /// Statistics for observability.
    pub fn stats(&self) -> GraphStats {
        let mut evaluated = 0;
        let mut failed = 0;
        let mut pending = 0;
        let mut has_tla = 0;

        for record in self.nodes.values() {
            match &record.status {
                ModuleStatus::Evaluated => evaluated += 1,
                ModuleStatus::Failed(_) => failed += 1,
                _ => pending += 1,
            }
            if record.has_top_level_await { has_tla += 1; }
        }

        GraphStats {
            total: self.nodes.len(),
            evaluated,
            failed,
            pending,
            has_top_level_await: has_tla,
            edge_count: self.edges.values().map(|v| v.len()).sum(),
        }
    }
}

impl Default for ModuleGraph {
    fn default() -> Self { Self::new() }
}

#[derive(Debug, Clone)]
pub struct GraphStats {
    pub total: usize,
    pub evaluated: usize,
    pub failed: usize,
    pub pending: usize,
    pub has_top_level_await: usize,
    pub edge_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::record::ModuleRecord;

    fn make_record(path: &str) -> ModuleRecord {
        let mut r = ModuleRecord::new(
            PathBuf::from(path),
            path.to_string(),
            String::new(),
        );
        r.status = super::super::record::ModuleStatus::Linked;
        r
    }

    #[test]
    fn linear_dependency_order() {
        // a imports b imports c
        let mut graph = ModuleGraph::new();
        graph.insert(make_record("/a.js"));
        graph.insert(make_record("/b.js"));
        graph.insert(make_record("/c.js"));
        graph.add_edge(&PathBuf::from("/a.js"), &PathBuf::from("/b.js"));
        graph.add_edge(&PathBuf::from("/b.js"), &PathBuf::from("/c.js"));

        let order = graph.eval_order(&PathBuf::from("/a.js")).unwrap();
        let paths: Vec<&str> = order.iter()
            .map(|p| p.to_str().unwrap())
            .collect();

        // c must come before b, b before a
        let c_pos = paths.iter().position(|&p| p == "/c.js").unwrap();
        let b_pos = paths.iter().position(|&p| p == "/b.js").unwrap();
        let a_pos = paths.iter().position(|&p| p == "/a.js").unwrap();
        assert!(c_pos < b_pos);
        assert!(b_pos < a_pos);
    }

    #[test]
    fn cycle_does_not_panic() {
        // a imports b imports a (cycle)
        let mut graph = ModuleGraph::new();
        graph.insert(make_record("/a.js"));
        graph.insert(make_record("/b.js"));
        graph.add_edge(&PathBuf::from("/a.js"), &PathBuf::from("/b.js"));
        graph.add_edge(&PathBuf::from("/b.js"), &PathBuf::from("/a.js"));

        // Should not panic or infinite loop
        let order = graph.eval_order(&PathBuf::from("/a.js")).unwrap();
        assert!(order.len() > 0);
    }

    #[test]
    fn diamond_dependency_deduped() {
        // a -> b, a -> c, b -> d, c -> d
        let mut graph = ModuleGraph::new();
        for p in &["/a.js", "/b.js", "/c.js", "/d.js"] {
            graph.insert(make_record(p));
        }
        graph.add_edge(&PathBuf::from("/a.js"), &PathBuf::from("/b.js"));
        graph.add_edge(&PathBuf::from("/a.js"), &PathBuf::from("/c.js"));
        graph.add_edge(&PathBuf::from("/b.js"), &PathBuf::from("/d.js"));
        graph.add_edge(&PathBuf::from("/c.js"), &PathBuf::from("/d.js"));

        let order = graph.eval_order(&PathBuf::from("/a.js")).unwrap();
        // d should appear exactly once
        let d_count = order.iter().filter(|p| p.to_str() == Some("/d.js")).count();
        assert_eq!(d_count, 1);
        assert_eq!(order.len(), 4);
    }

    #[test]
    fn missing_entry_errors() {
        let graph = ModuleGraph::new();
        assert!(graph.eval_order(&PathBuf::from("/nonexistent.js")).is_err());
    }
}
