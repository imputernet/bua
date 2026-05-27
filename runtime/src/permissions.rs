/// permissions.rs — Runtime permission enforcement
///
/// PermissionGuard wraps a CapabilitySet and provides auditable
/// check-or-deny semantics with structured logging.
use bua_core::{BuaError, BuaResult, CapabilitySet, Permission};
use parking_lot::RwLock;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct PermissionGuard {
    caps: Arc<RwLock<CapabilitySet>>,
    /// Optional audit log sink. In production: write to trace buffer.
    audit_enabled: bool,
}

impl PermissionGuard {
    pub fn new(caps: CapabilitySet) -> Self {
        Self {
            caps: Arc::new(RwLock::new(caps)),
            audit_enabled: true,
        }
    }

    /// Check a permission, returning Ok(()) or a typed error.
    pub fn require(&self, perm: &Permission) -> BuaResult<()> {
        let granted = self.caps.read().check(perm);

        if self.audit_enabled {
            tracing::debug!(
                permission = ?perm,
                granted,
                "permission check"
            );
        }

        if granted {
            Ok(())
        } else {
            Err(BuaError::permission_denied(
                format!("{perm:?}"),
                "no matching capability",
            ))
        }
    }

    /// Require permission; if denied, call the provided fallback instead of erroring.
    pub fn require_or<F>(&self, perm: &Permission, fallback: F) -> BuaResult<()>
    where
        F: FnOnce() -> BuaResult<()>,
    {
        if self.caps.read().check(perm) {
            Ok(())
        } else {
            fallback()
        }
    }

    /// Revoke a capability at runtime.
    pub fn revoke(&self, cap: &bua_core::Capability) {
        let removed = self.caps.write().revoke(cap);
        tracing::warn!(
            capability = ?cap,
            removed,
            generation = self.caps.read().generation(),
            "capability revoked"
        );
    }

    /// Replace the entire capability set atomically.
    pub fn replace(&self, new_caps: CapabilitySet) {
        *self.caps.write() = new_caps;
    }

    pub fn snapshot_caps(&self) -> CapabilitySet {
        self.caps.read().clone()
    }
}
