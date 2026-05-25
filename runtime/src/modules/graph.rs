use bua_core::{BuaError, BuaResult};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct EvalOrder(pub Vec<PathBuf>);
impl EvalOrder {
    pub fn iter(&self) -> impl Iterator<Item = &Path> {
        self.0.iter().map(|p| p.as_path())
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

pub struct ModuleGraph {
    nodes: HashMap<String, ModuleRecord>,
    edges: HashMap<String, Vec<String>>,
}
use super::record::{ModuleRecord, ModuleStatus};

impl ModuleGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
        }
    }
    pub fn insert(&mut self, record: ModuleRecord) {
        let key = record.resolved_path.to_string_lossy().into_owned();
        self.edges.insert(key.clone(), Vec::new());
        self.nodes.insert(key, record);
    }
    pub fn add_edge(&mut self, from: &Path, to: &Path) {
        let from_key = from.to_string_lossy().into_owned();
        let to_key = to.to_string_lossy().into_owned();
        self.edges.entry(from_key).or_default().push(to_key);
    }
    pub fn get(&self, path: &Path) -> Option<&ModuleRecord> {
        self.nodes.get(&path.to_string_lossy().into_owned())
    }
    pub fn get_mut(&mut self, path: &Path) -> Option<&mut ModuleRecord> {
        self.nodes.get_mut(&path.to_string_lossy().into_owned())
    }
    pub fn contains(&self, path: &Path) -> bool {
        self.nodes
            .contains_key(&path.to_string_lossy().into_owned())
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn eval_order(&self, entry: &Path) -> BuaResult<EvalOrder> {
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
            tracing::warn!(cycles = cycles.join(" -> "), "ESM cyclic imports detected");
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
            return;
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
    pub fn failed_modules(&self) -> Vec<(&str, &str)> {
        self.nodes
            .iter()
            .filter_map(|(k, v)| v.status.failed_reason().map(|r| (k.as_str(), r)))
            .collect()
    }
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
            if record.has_top_level_await {
                has_tla += 1;
            }
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
    fn default() -> Self {
        Self::new()
    }
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
        let mut r = ModuleRecord::new(PathBuf::from(path), path.to_string(), String::new());
        r.status = super::super::record::ModuleStatus::Linked;
        r
    }
    #[test]
    fn linear_dependency_order() {
        let mut graph = ModuleGraph::new();
        graph.insert(make_record("/a.js"));
        graph.insert(make_record("/b.js"));
        graph.insert(make_record("/c.js"));
        graph.add_edge(Path::new("/a.js"), Path::new("/b.js"));
        graph.add_edge(Path::new("/b.js"), Path::new("/c.js"));
        let order = graph.eval_order(Path::new("/a.js")).unwrap();
        let paths: Vec<&str> = order.iter().map(|p| p.to_str().unwrap()).collect();
        let c_pos = paths.iter().position(|&p| p == "/c.js").unwrap();
        let b_pos = paths.iter().position(|&p| p == "/b.js").unwrap();
        let a_pos = paths.iter().position(|&p| p == "/a.js").unwrap();
        assert!(c_pos < b_pos);
        assert!(b_pos < a_pos);
    }
    #[test]
    fn cycle_does_not_panic() {
        let mut graph = ModuleGraph::new();
        graph.insert(make_record("/a.js"));
        graph.insert(make_record("/b.js"));
        graph.add_edge(Path::new("/a.js"), Path::new("/b.js"));
        graph.add_edge(Path::new("/b.js"), Path::new("/a.js"));
        let order = graph.eval_order(Path::new("/a.js")).unwrap();
        assert!(!order.is_empty());
    }
    #[test]
    fn diamond_dependency_deduped() {
        let mut graph = ModuleGraph::new();
        for p in &["/a.js", "/b.js", "/c.js", "/d.js"] {
            graph.insert(make_record(p));
        }
        graph.add_edge(Path::new("/a.js"), Path::new("/b.js"));
        graph.add_edge(Path::new("/a.js"), Path::new("/c.js"));
        graph.add_edge(Path::new("/b.js"), Path::new("/d.js"));
        graph.add_edge(Path::new("/c.js"), Path::new("/d.js"));
        let order = graph.eval_order(Path::new("/a.js")).unwrap();
        let d_count = order.iter().filter(|p| p.to_str() == Some("/d.js")).count();
        assert_eq!(d_count, 1);
        assert_eq!(order.len(), 4);
    }
    #[test]
    fn missing_entry_errors() {
        let graph = ModuleGraph::new();
        assert!(graph.eval_order(Path::new("/nonexistent.js")).is_err());
    }
}
