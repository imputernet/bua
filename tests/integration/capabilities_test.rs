// tests/integration/capabilities_test.rs
// Tests that the permission system correctly allows/denies operations.

#[cfg(test)]
mod capability_enforcement {
    use bua_core::{
        Capability, CapabilitySet, FsCapability, FsMode, NetCapability, Permission,
    };
    use std::path::PathBuf;

    fn workspace_caps() -> CapabilitySet {
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::Filesystem(FsCapability {
            allowed_roots: vec![PathBuf::from("/tmp/workspace")],
            mode: FsMode::READ | FsMode::WRITE | FsMode::CREATE,
        }));
        caps.grant(Capability::Network(NetCapability {
            allowed_hosts: vec!["api.openai.com".into()],
            allow_outbound: true,
            allow_inbound: false,
        }));
        caps
    }

    #[test]
    fn allows_reads_within_workspace() {
        let caps = workspace_caps();
        assert!(caps.check(&Permission::FsRead(PathBuf::from("/tmp/workspace/data.json"))));
    }

    #[test]
    fn denies_reads_outside_workspace() {
        let caps = workspace_caps();
        assert!(!caps.check(&Permission::FsRead(PathBuf::from("/etc/shadow"))));
    }

    #[test]
    fn denies_writes_when_only_read_granted() {
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::Filesystem(FsCapability {
            allowed_roots: vec![PathBuf::from("/tmp/ro")],
            mode: FsMode::READ,
        }));
        assert!(caps.check(&Permission::FsRead(PathBuf::from("/tmp/ro/file"))));
        assert!(!caps.check(&Permission::FsWrite(PathBuf::from("/tmp/ro/file"))));
    }

    #[test]
    fn allows_exact_host() {
        let caps = workspace_caps();
        assert!(caps.check(&Permission::NetConnect("api.openai.com".into())));
    }

    #[test]
    fn denies_unlisted_host() {
        let caps = workspace_caps();
        assert!(!caps.check(&Permission::NetConnect("evil.example.com".into())));
    }

    #[test]
    fn wildcard_host_matches_subdomain() {
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::Network(NetCapability {
            allowed_hosts: vec!["*.anthropic.com".into()],
            allow_outbound: true,
            allow_inbound: false,
        }));
        assert!(caps.check(&Permission::NetConnect("api.anthropic.com".into())));
        assert!(caps.check(&Permission::NetConnect("claude.anthropic.com".into())));
        assert!(!caps.check(&Permission::NetConnect("anthropic.com".into())));
    }

    #[test]
    fn revocation_increments_generation_and_denies() {
        let mut caps = workspace_caps();
        let gen_before = caps.generation();

        let net_cap = Capability::Network(NetCapability {
            allowed_hosts: vec!["api.openai.com".into()],
            allow_outbound: true,
            allow_inbound: false,
        });

        let removed = caps.revoke(&net_cap);
        assert!(removed);
        assert_eq!(caps.generation(), gen_before + 1);
        assert!(!caps.check(&Permission::NetConnect("api.openai.com".into())));
    }

    #[test]
    fn unrestricted_passes_all_checks() {
        let caps = CapabilitySet::unrestricted();
        assert!(caps.check(&Permission::FsRead(PathBuf::from("/etc/passwd"))));
        assert!(caps.check(&Permission::NetConnect("anywhere.com".into())));
        assert!(caps.check(&Permission::AgentSpawn));
    }

    #[test]
    fn no_caps_denies_everything() {
        let caps = CapabilitySet::new();
        assert!(!caps.check(&Permission::FsRead(PathBuf::from("/tmp/x"))));
        assert!(!caps.check(&Permission::NetConnect("a.com".into())));
        assert!(!caps.check(&Permission::AgentSpawn));
    }
}
