//! Contract-name index — Phase 5.
//!
//! Absorbs the Phase 4 `kernel::registry` responsibility
//! (manifest dedup + contract-name → provider lookup). The
//! resolver builds this once and feeds it to the topo pass.
//!
//! The index returns the publishing `(PluginId, manifest, cap_name)`
//! tuple so downstream phases can carry the cap name forward
//! without re-walking every manifest.

use std::collections::BTreeMap;

use crate::host::manifest::PluginManifest;
use crate::kernel::ids::PluginId;

use super::error::ResolveError;

/// Single contract-name index entry. The cap name is the
/// `[[exposes]] name` field of the provider — distinct from
/// the contract name itself (which is the resolver key).
#[derive(Debug, Clone)]
pub(crate) struct ContractEntry<'a> {
    pub plugin: PluginId,
    #[allow(dead_code)]
    pub manifest: &'a PluginManifest,
    pub cap_name: &'a str,
}

/// Build a contract-name index over every manifest. Detects
/// two failure modes at construction time:
///  - `DuplicateName`: two manifests share the same `(name, version)`.
///  - `Ambiguous`: two manifests publish the same `contract_name`.
///
/// Both are boot errors — the caller surfaces them before
/// any plugin starts minting. Detecting at index-build time
/// is what lets the rest of the resolver assume "every
/// contract has exactly one provider" and run a single
/// hash-table lookup per `requires` entry.
pub(crate) fn build_contract_index(
    manifests: &[PluginManifest],
) -> Result<(BTreeMap<String, ContractEntry<'_>>, BTreeMap<PluginId, &PluginManifest>), ResolveError> {
    let mut by_contract: BTreeMap<String, ContractEntry<'_>> = BTreeMap::new();
    let mut by_plugin: BTreeMap<PluginId, &PluginManifest> = BTreeMap::new();

    for m in manifests {
        let pid = m.plugin.clone();
        if by_plugin.insert(pid.clone(), m).is_some() {
            return Err(ResolveError::DuplicateName { plugin: pid });
        }
        for e in &m.exposes {
            if e.contract_name.is_empty() {
                continue;
            }
            if by_contract.contains_key(&e.contract_name) {
                let other = by_contract.get(&e.contract_name).unwrap();
                return Err(ResolveError::Ambiguous {
                    contract: e.contract_name.clone(),
                    a: format!("{}@{}", other.plugin.name, other.plugin.version),
                    b: format!("{}@{}", pid.name, pid.version),
                });
            }
            by_contract.insert(
                e.contract_name.clone(),
                ContractEntry {
                    plugin: pid.clone(),
                    manifest: m,
                    cap_name: &e.name,
                },
            );
        }
    }

    Ok((by_contract, by_plugin))
}
