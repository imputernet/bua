// runtime/src/snapshot.rs
//
// Layered snapshot format.
//
// Format on disk:
//   [MAGIC: 8 bytes] [VERSION: u16 LE]
//   [STRATUM_TAG: u16 LE] [STRATUM_LEN: u32 LE] [STRATUM_DATA: bytes]
//   ... (repeated for each stratum)
//   [CRC32: u32 LE]
//
// Strata tags:
//   0x0000  Header (always present)
//   0x0001  VmStratum       — JSC heap bytecode
//   0x0002  CapabilityStratum
//   0x0003  TraceStratum
//   0x0004  ToolStratum
//   0x0005  SchedulerStratum
//   0x0006  MemoryStratum
//
// Unknown tags are skipped (forward compatibility).

use bua_core::{BuaError, BuaResult, CapabilitySet, ExecutionId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

const MAGIC: &[u8; 8] = b"BUASNAP\x02";
const FORMAT_VERSION: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum StratumTag {
    Vm = 0x0001,
    Capability = 0x0002,
    Trace = 0x0003,
    Tool = 0x0004,
    Scheduler = 0x0005,
    Memory = 0x0006,
}

impl StratumTag {
    fn from_u16(v: u16) -> Option<Self> {
        match v {
            0x0001 => Some(Self::Vm),
            0x0002 => Some(Self::Capability),
            0x0003 => Some(Self::Trace),
            0x0004 => Some(Self::Tool),
            0x0005 => Some(Self::Scheduler),
            0x0006 => Some(Self::Memory),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmStratum {
    pub heap_bytes: Vec<u8>,
    pub jsc_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityStratum {
    pub capabilities: CapabilitySet,
    pub generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceStratum {
    pub ndjson: String,
    pub event_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStratum {
    pub call_log: Vec<ToolCallRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub sequence: u64,
    pub name: String,
    pub args_json: String,
    pub result_json: String,
    pub duration_us: u64,
    pub was_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerStratum {
    pub child_agent_ids: Vec<String>,
    pub pending_task_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStratum {
    pub entries: HashMap<String, serde_json::Value>,
    pub namespace: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotHeader {
    pub execution_id: ExecutionId,
    pub timestamp_us: u64,
    pub format_version: u16,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct LayeredSnapshot {
    pub header: Option<SnapshotHeader>,
    pub vm: Option<VmStratum>,
    pub capability: Option<CapabilityStratum>,
    pub trace: Option<TraceStratum>,
    pub tool: Option<ToolStratum>,
    pub scheduler: Option<SchedulerStratum>,
    pub memory: Option<MemoryStratum>,
}

impl LayeredSnapshot {
    pub fn new(execution_id: ExecutionId) -> Self {
        let timestamp_us = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        Self {
            header: Some(SnapshotHeader {
                execution_id,
                timestamp_us,
                format_version: FORMAT_VERSION,
                label: None,
            }),
            ..Default::default()
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        if let Some(h) = &mut self.header {
            h.label = Some(label.into());
        }
        self
    }
    pub fn with_vm(mut self, heap_bytes: Vec<u8>) -> Self {
        self.vm = Some(VmStratum {
            heap_bytes,
            jsc_version: "JSC/stub-0.1".into(),
        });
        self
    }
    pub fn with_capability(mut self, capabilities: CapabilitySet, generation: u64) -> Self {
        self.capability = Some(CapabilityStratum {
            capabilities,
            generation,
        });
        self
    }
    pub fn with_trace(mut self, ndjson: String, event_count: u64) -> Self {
        self.trace = Some(TraceStratum {
            ndjson,
            event_count,
        });
        self
    }
    pub fn with_tool_log(mut self, call_log: Vec<ToolCallRecord>) -> Self {
        self.tool = Some(ToolStratum { call_log });
        self
    }
    pub fn with_scheduler(mut self, child_agent_ids: Vec<String>, pending_task_count: u64) -> Self {
        self.scheduler = Some(SchedulerStratum {
            child_agent_ids,
            pending_task_count,
        });
        self
    }
    pub fn with_memory(
        mut self,
        entries: HashMap<String, serde_json::Value>,
        namespace: String,
    ) -> Self {
        self.memory = Some(MemoryStratum { entries, namespace });
        self
    }

    pub fn has_vm(&self) -> bool {
        self.vm.is_some()
    }
    pub fn has_trace(&self) -> bool {
        self.trace.is_some()
    }
    pub fn has_capability(&self) -> bool {
        self.capability.is_some()
    }
    pub fn has_memory(&self) -> bool {
        self.memory.is_some()
    }

    pub fn strata_count(&self) -> usize {
        [
            self.vm.is_some(),
            self.capability.is_some(),
            self.trace.is_some(),
            self.tool.is_some(),
            self.scheduler.is_some(),
            self.memory.is_some(),
        ]
        .iter()
        .filter(|&&b| b)
        .count()
    }

    pub fn serialize(&self) -> BuaResult<Vec<u8>> {
        let mut out = Vec::with_capacity(4096);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        let header_bytes = serde_json::to_vec(&self.header).map_err(BuaError::Serialize)?;
        write_stratum_raw(&mut out, 0x0000, &header_bytes);
        if let Some(ref s) = self.vm {
            write_stratum(&mut out, StratumTag::Vm, s)?;
        }
        if let Some(ref s) = self.capability {
            write_stratum(&mut out, StratumTag::Capability, s)?;
        }
        if let Some(ref s) = self.trace {
            write_stratum(&mut out, StratumTag::Trace, s)?;
        }
        if let Some(ref s) = self.tool {
            write_stratum(&mut out, StratumTag::Tool, s)?;
        }
        if let Some(ref s) = self.scheduler {
            write_stratum(&mut out, StratumTag::Scheduler, s)?;
        }
        if let Some(ref s) = self.memory {
            write_stratum(&mut out, StratumTag::Memory, s)?;
        }
        let crc = crc32_simple(&out);
        out.extend_from_slice(&crc.to_le_bytes());
        Ok(out)
    }

    pub fn deserialize(data: &[u8]) -> BuaResult<Self> {
        if data.len() < 14 {
            return Err(BuaError::SnapshotRestore("too short".into()));
        }
        if &data[..8] != MAGIC {
            return Err(BuaError::SnapshotRestore("invalid magic".to_string()));
        }
        let crc_offset = data.len() - 4;
        let stored_crc = u32::from_le_bytes(data[crc_offset..].try_into().unwrap());
        let computed = crc32_simple(&data[..crc_offset]);
        if stored_crc != computed {
            return Err(BuaError::SnapshotRestore(format!(
                "CRC mismatch: stored={stored_crc:#010x} computed={computed:#010x}"
            )));
        }
        let version = u16::from_le_bytes(data[8..10].try_into().unwrap());
        if version > FORMAT_VERSION {
            return Err(BuaError::SnapshotRestore(format!(
                "unsupported version {version}"
            )));
        }
        let mut snap = LayeredSnapshot::default();
        let mut pos = 10usize;
        while pos + 6 <= crc_offset {
            let tag = u16::from_le_bytes(data[pos..pos + 2].try_into().unwrap());
            let len = u32::from_le_bytes(data[pos + 2..pos + 6].try_into().unwrap()) as usize;
            pos += 6;
            if pos + len > crc_offset {
                return Err(BuaError::SnapshotRestore("stratum overflow".into()));
            }
            let payload = &data[pos..pos + len];
            pos += len;
            match tag {
                0x0000 => {
                    snap.header = serde_json::from_slice(payload).ok();
                }
                t => match StratumTag::from_u16(t) {
                    Some(StratumTag::Vm) => {
                        snap.vm = serde_json::from_slice(payload).ok();
                    }
                    Some(StratumTag::Capability) => {
                        snap.capability = serde_json::from_slice(payload).ok();
                    }
                    Some(StratumTag::Trace) => {
                        snap.trace = serde_json::from_slice(payload).ok();
                    }
                    Some(StratumTag::Tool) => {
                        snap.tool = serde_json::from_slice(payload).ok();
                    }
                    Some(StratumTag::Scheduler) => {
                        snap.scheduler = serde_json::from_slice(payload).ok();
                    }
                    Some(StratumTag::Memory) => {
                        snap.memory = serde_json::from_slice(payload).ok();
                    }
                    None => {
                        tracing::debug!(tag = t, "unknown stratum skipped");
                    }
                },
            }
        }
        Ok(snap)
    }

    pub async fn write_to_file(&self, path: &Path) -> BuaResult<()> {
        let bytes = self.serialize()?;
        tokio::fs::write(path, &bytes).await.map_err(BuaError::Io)?;
        tracing::info!(path = %path.display(), bytes = bytes.len(), strata = self.strata_count(), "snapshot written");
        Ok(())
    }

    pub async fn read_from_file(path: &Path) -> BuaResult<Self> {
        let data = tokio::fs::read(path).await.map_err(BuaError::Io)?;
        let snap = Self::deserialize(&data)?;
        tracing::info!(path = %path.display(), strata = snap.strata_count(), "snapshot loaded");
        Ok(snap)
    }
}

fn write_stratum<S: Serialize>(out: &mut Vec<u8>, tag: StratumTag, s: &S) -> BuaResult<()> {
    let bytes = serde_json::to_vec(s).map_err(BuaError::Serialize)?;
    write_stratum_raw(out, tag as u16, &bytes);
    Ok(())
}

fn write_stratum_raw(out: &mut Vec<u8>, tag: u16, bytes: &[u8]) {
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
}

fn crc32_simple(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc ^ 0xFFFF_FFFF
}

// Legacy snapshot kept for CLI compat during migration
#[derive(Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub execution_id: ExecutionId,
    pub timestamp_us: u64,
    pub capabilities: CapabilitySet,
    pub trace_ndjson: String,
    pub heap_bytes: Vec<u8>,
    pub pending_tool_calls: Vec<serde_json::Value>,
}

impl Snapshot {
    pub fn new(
        execution_id: ExecutionId,
        capabilities: CapabilitySet,
        trace_ndjson: String,
        heap_bytes: Vec<u8>,
    ) -> Self {
        Self {
            version: 1,
            execution_id,
            timestamp_us: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64,
            capabilities,
            trace_ndjson,
            heap_bytes,
            pending_tool_calls: Vec::new(),
        }
    }
    pub fn upgrade(self) -> LayeredSnapshot {
        LayeredSnapshot::new(self.execution_id)
            .with_vm(self.heap_bytes)
            .with_capability(self.capabilities, 0)
            .with_trace(self.trace_ndjson, 0)
    }
    pub async fn write_to_file(&self, path: &Path) -> BuaResult<()> {
        let bytes = serde_json::to_vec(self).map_err(BuaError::Serialize)?;
        tokio::fs::write(path, &bytes).await.map_err(BuaError::Io)
    }
    pub async fn read_from_file(path: &Path) -> BuaResult<Self> {
        let data = tokio::fs::read(path).await.map_err(BuaError::Io)?;
        serde_json::from_slice(&data).map_err(BuaError::Serialize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bua_core::ExecutionId;
    use tempfile::TempDir;

    fn full_snap() -> LayeredSnapshot {
        LayeredSnapshot::new(ExecutionId::new())
            .with_label("test")
            .with_vm(vec![0xDE, 0xAD])
            .with_capability(CapabilitySet::new(), 0)
            .with_trace(r#"{"id":0}"#.into(), 1)
            .with_tool_log(vec![ToolCallRecord {
                sequence: 0,
                name: "t".into(),
                args_json: "{}".into(),
                result_json: "{}".into(),
                duration_us: 100,
                was_error: false,
            }])
            .with_scheduler(vec![], 0)
            .with_memory([("k".into(), serde_json::json!("v"))].into(), "ns".into())
    }

    #[test]
    fn roundtrip() {
        let snap = full_snap();
        assert_eq!(snap.strata_count(), 6);
        let bytes = snap.serialize().unwrap();
        let loaded = LayeredSnapshot::deserialize(&bytes).unwrap();
        assert_eq!(loaded.strata_count(), 6);
        assert_eq!(loaded.vm.unwrap().heap_bytes, vec![0xDE, 0xAD]);
        assert_eq!(loaded.header.unwrap().label.as_deref(), Some("test"));
    }

    #[test]
    fn crc_detect() {
        let mut bytes = full_snap().serialize().unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0xFF;
        assert!(LayeredSnapshot::deserialize(&bytes)
            .unwrap_err()
            .to_string()
            .contains("CRC"));
    }

    #[test]
    fn wrong_magic() {
        let mut bytes = full_snap().serialize().unwrap();
        bytes[0] = 0xFF;
        assert!(LayeredSnapshot::deserialize(&bytes).is_err());
    }

    #[test]
    fn partial_snap() {
        let snap = LayeredSnapshot::new(ExecutionId::new()).with_trace("{}".into(), 0);
        let bytes = snap.serialize().unwrap();
        let loaded = LayeredSnapshot::deserialize(&bytes).unwrap();
        assert!(loaded.vm.is_none());
        assert!(loaded.trace.is_some());
    }

    #[tokio::test]
    async fn file_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("a.bsnap");
        full_snap().write_to_file(&path).await.unwrap();
        let loaded = LayeredSnapshot::read_from_file(&path).await.unwrap();
        assert_eq!(loaded.strata_count(), 6);
    }

    #[test]
    fn legacy_upgrade() {
        let old = Snapshot::new(
            ExecutionId::new(),
            CapabilitySet::new(),
            "{}\n".into(),
            vec![1, 2],
        );
        let new = old.upgrade();
        assert!(new.has_vm() && new.has_trace() && new.has_capability());
    }
}
