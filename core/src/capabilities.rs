use bitflags::bitflags;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

/// A single granted capability with its scope.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    /// Filesystem access scoped to a set of allowed roots.
    Filesystem(FsCapability),
    /// Network access scoped to allowed hosts/ports.
    Network(NetCapability),
    /// Subprocess execution, scoped to allowed executables.
    Subprocess(SubprocessCapability),
    /// Environment variable read access.
    EnvRead(EnvCapability),
    /// Agent spawning — can create child agents.
    AgentSpawn,
    /// Full system access (dangerous; only for trusted root agents).
    Unrestricted,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FsCapability {
    pub allowed_roots: Vec<PathBuf>,
    pub mode: FsMode,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct FsMode: u8 {
        const READ    = 0b0001;
        const WRITE   = 0b0010;
        const CREATE  = 0b0100;
        const DELETE  = 0b1000;
        const READ_WRITE = Self::READ.bits() | Self::WRITE.bits() | Self::CREATE.bits();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetCapability {
    /// e.g. "openai.com", "*.anthropic.com", "127.0.0.1:8080"
    pub allowed_hosts: Vec<String>,
    pub allow_outbound: bool,
    pub allow_inbound: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SubprocessCapability {
    /// Allowed executable names (basename only, no path traversal).
    pub allowed_executables: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EnvCapability {
    /// Allowed env var names; empty = all allowed.
    pub allowed_keys: Vec<String>,
}

/// The full set of capabilities granted to an execution context.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilitySet {
    caps: HashSet<Capability>,
    /// Monotonic revocation generation. Incremented on any revoke.
    generation: u64,
}

impl CapabilitySet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn unrestricted() -> Self {
        let mut set = Self::new();
        set.caps.insert(Capability::Unrestricted);
        set
    }

    pub fn grant(&mut self, cap: Capability) {
        self.caps.insert(cap);
    }

    pub fn revoke(&mut self, cap: &Capability) -> bool {
        let removed = self.caps.remove(cap);
        if removed {
            self.generation += 1;
        }
        removed
    }

    pub fn revoke_all(&mut self) {
        self.caps.clear();
        self.generation += 1;
    }

    /// Returns true if the requested permission is satisfied by any granted capability.
    pub fn check(&self, permission: &Permission) -> bool {
        if self.caps.contains(&Capability::Unrestricted) {
            return true;
        }
        match permission {
            Permission::FsRead(path) => self.caps.iter().any(|c| {
                if let Capability::Filesystem(fs) = c {
                    fs.mode.contains(FsMode::READ)
                        && fs.allowed_roots.iter().any(|root| path.starts_with(root))
                } else {
                    false
                }
            }),
            Permission::FsWrite(path) => self.caps.iter().any(|c| {
                if let Capability::Filesystem(fs) = c {
                    fs.mode.contains(FsMode::WRITE)
                        && fs.allowed_roots.iter().any(|root| path.starts_with(root))
                } else {
                    false
                }
            }),
            Permission::NetConnect(host) => self.caps.iter().any(|c| {
                if let Capability::Network(net) = c {
                    net.allow_outbound && net.allowed_hosts.iter().any(|h| host_matches(h, host))
                } else {
                    false
                }
            }),
            Permission::Subprocess(exe) => self.caps.iter().any(|c| {
                if let Capability::Subprocess(sub) = c {
                    sub.allowed_executables.iter().any(|e| e == exe || e == "*")
                } else {
                    false
                }
            }),
            Permission::EnvRead(key) => self.caps.iter().any(|c| {
                if let Capability::EnvRead(env) = c {
                    env.allowed_keys.is_empty() || env.allowed_keys.iter().any(|k| k == key)
                } else {
                    false
                }
            }),
            Permission::AgentSpawn => self.caps.contains(&Capability::AgentSpawn),
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn iter(&self) -> impl Iterator<Item = &Capability> {
        self.caps.iter()
    }
}

/// A permission request — what an operation wants to do.
#[derive(Debug, Clone)]
pub enum Permission {
    FsRead(PathBuf),
    FsWrite(PathBuf),
    NetConnect(String),
    Subprocess(String),
    EnvRead(String),
    AgentSpawn,
}

/// Glob-style host matching: "*.openai.com" matches "api.openai.com".
fn host_matches(pattern: &str, host: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        host == suffix || host.ends_with(&format!(".{suffix}"))
    } else {
        pattern == host
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fs_read_allowed_within_root() {
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::Filesystem(FsCapability {
            allowed_roots: vec![PathBuf::from("/workspace")],
            mode: FsMode::READ,
        }));
        assert!(caps.check(&Permission::FsRead(PathBuf::from("/workspace/foo.ts"))));
        assert!(!caps.check(&Permission::FsRead(PathBuf::from("/etc/passwd"))));
    }

    #[test]
    fn net_wildcard_matching() {
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::Network(NetCapability {
            allowed_hosts: vec!["*.openai.com".into()],
            allow_outbound: true,
            allow_inbound: false,
        }));
        assert!(caps.check(&Permission::NetConnect("api.openai.com".into())));
        assert!(!caps.check(&Permission::NetConnect("evil.com".into())));
    }

    #[test]
    fn revoke_increments_generation() {
        let mut caps = CapabilitySet::new();
        let cap = Capability::AgentSpawn;
        caps.grant(cap.clone());
        assert_eq!(caps.generation(), 0);
        caps.revoke(&cap);
        assert_eq!(caps.generation(), 1);
        assert!(!caps.check(&Permission::AgentSpawn));
    }
}
