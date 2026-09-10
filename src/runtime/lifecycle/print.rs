//! Manifest table formatter.
//!
//! Prints a 4-column table (plugin, exposes, contract, requires)
//! with column widths computed from the data so the boot log
//! stays readable regardless of plugin name length.

use crate::host::manifest::PluginManifest;

/// Print every loaded manifest so operators can confirm the
/// resolver's input.
pub fn print_manifests(manifests: &[PluginManifest]) {
    let mut rows: Vec<(String, String, String, String)> = Vec::with_capacity(manifests.len());
    for m in manifests {
        let name_ver = format!("{}@{}", m.plugin.name, m.plugin.version);
        let exposes = m
            .exposes
            .iter()
            .map(|c| c.name.clone())
            .collect::<Vec<_>>()
            .join(",");
        let contracts = m
            .exposes
            .iter()
            .map(|c| c.contract_name.clone())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        let requires = if m.requires.is_empty() {
            "—".to_string()
        } else {
            m.requires
                .iter()
                .map(|r| format!("{}→{}", r.name, r.contract))
                .collect::<Vec<_>>()
                .join(",")
        };
        rows.push((name_ver, exposes, contracts, requires));
    }

    let headers = (
        "plugin".to_string(),
        "exposes".to_string(),
        "contract".to_string(),
        "requires".to_string(),
    );
    let col_width = |label: &str, cells: &[String]| -> usize {
        cells
            .iter()
            .map(|s| s.chars().count())
            .max()
            .unwrap_or(0)
            .max(label.chars().count())
            + 1
    };
    let wp = col_width(&headers.0, &rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>());
    let we = col_width(&headers.1, &rows.iter().map(|r| r.1.clone()).collect::<Vec<_>>());
    let wc = col_width(&headers.2, &rows.iter().map(|r| r.2.clone()).collect::<Vec<_>>());
    let wr = col_width(&headers.3, &rows.iter().map(|r| r.3.clone()).collect::<Vec<_>>());

    println!("[manifest] loaded {} plugin(s):", manifests.len());
    println!(
        "  {:<wp$}{:<we$}{:<wc$}{:<wr$}",
        headers.0, headers.1, headers.2, headers.3,
    );
    println!(
        "  {}{}{}{}",
        "-".repeat(wp - 1),
        "-".repeat(we),
        "-".repeat(wc),
        "-".repeat(wr),
    );
    for (n, e, c, r) in &rows {
        println!(
            "  {:<wp$}{:<we$}{:<wc$}{:<wr$}",
            n, e, c, r,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::manifest::{
        CapabilityDecl, CapabilityRequirement, IsolationMode, PluginManifest, ResourceHints,
    };
    use crate::kernel::ids::PluginId;
    use crate::kernel::meta::{AuthorityContract, Protocol};

    fn manifest(name: &str, version: &str, requires: Vec<(&str, &str)>) -> PluginManifest {
        PluginManifest {
            plugin: PluginId {
                name: name.into(),
                version: version.into(),
            },
            isolate: IsolationMode::InProc,
            exposes: vec![CapabilityDecl {
                name: name.into(),
                in_type: "any".into(),
                out_type: "any".into(),
                streaming: false,
                contract_name: name.into(),
                authority: AuthorityContract::empty(),
                protocol: Protocol::empty(),
            }],
            requires: requires
                .into_iter()
                .map(|(h, c)| CapabilityRequirement {
                    name: h.into(),
                    contract: c.into(),
                })
                .collect(),
            host: Vec::new(),
            resources: ResourceHints::default(),
        }
    }

    #[test]
    fn print_manifests_does_not_panic_on_empty() {
        print_manifests(&[]);
    }

    #[test]
    fn print_manifests_handles_long_plugin_names() {
        // 200-char plugin name — exercises column-width math
        // and the format-string left-alignment without panicking.
        let long_name = "a".repeat(200);
        let m = manifest(&long_name, "0.1.0", vec![]);
        print_manifests(&[m]);
    }

    #[test]
    fn print_manifests_handles_many_requires() {
        // Many dependencies force the requires column wide.
        let reqs: Vec<(&str, &str)> = (0..20)
            .map(|_| ("handle", "contract_name_that_is_quite_long_too"))
            .collect();
        let m = manifest("alpha", "0.1.0", reqs);
        print_manifests(&[m]);
    }
}
