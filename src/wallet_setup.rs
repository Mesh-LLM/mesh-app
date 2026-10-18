//! The wallet is a detail: when someone first chooses to pay or get paid, the
//! app installs the pinned Lexe wallet plugin for them. Never at launch.
//! The archive is downloaded from its GitHub release and must match the
//! SHA-256 pinned here before anything is extracted.
use mesh_llm_sdk::plugin_install::install::{install_plugin_archive, PluginInstallOptions};
use mesh_llm_sdk::plugin_install::store::PluginStore;
use mesh_llm_sdk::plugin_install::PluginProgressEvent;
use sha2::{Digest, Sha256};
use std::io::Read;

pub const NAME: &str = "lexe-wallet";
pub const VERSION: &str = "0.1.0";
const MAX_ARCHIVE: u64 = 256 * 1024 * 1024;

/// `(target triple, SHA-256 of lexe-wallet v0.1.0's release archive)`.
const PINS: &[(&str, &str)] = &[
    (
        "aarch64-apple-darwin",
        "acd692e7b6cfa03ff51ee782bec8b9eec641f791ba9fdb65a81ac8080ce6433f",
    ),
    (
        "x86_64-apple-darwin",
        "cc95edd3a8440464847f7132b14ec869e831de97ca3a05fb8d61c227b267861b",
    ),
    (
        "aarch64-pc-windows-msvc",
        "b224f071c3304563fa4c09c41589e7ec95220a91fd59d4c3927e0e75a0536a46",
    ),
    (
        "x86_64-pc-windows-msvc",
        "43468986ac75f93e82520c31cb17b110583dd6724047d7ed12e175918729b540",
    ),
    (
        "aarch64-unknown-linux-gnu",
        "c2a612e1cf6c7823a2c5fcfcfe1af363a3a66e548a79f92a2a5aa3de25e97baf",
    ),
    (
        "x86_64-unknown-linux-gnu",
        "e565454cd5b8c8daf7e3091c1bffb9a30b205156c2a69cbe8c2c6f8f13589d64",
    ),
];

fn pin(triple: &str) -> Option<&'static str> {
    PINS.iter().find(|(t, _)| *t == triple).map(|(_, s)| *s)
}

fn archive_name(triple: &str) -> String {
    let ext = if triple.contains("windows") {
        "zip"
    } else {
        "tar.gz"
    };
    format!("{NAME}-v{VERSION}-{triple}.{ext}")
}

fn archive_url(triple: &str) -> String {
    format!(
        "https://github.com/Mesh-LLM/lexe-wallet/releases/download/v{VERSION}/{}",
        archive_name(triple)
    )
}

/// Any installed version counts: the operator may have installed one by hand.
pub fn installed() -> bool {
    PluginInstallOptions::from_env()
        .ok()
        .and_then(|o| PluginStore::new(o.store_root).try_load(NAME).ok().flatten())
        .is_some()
}

fn verify(bytes: &[u8], expected: &str) -> Result<(), String> {
    let actual = hex(&Sha256::digest(bytes));
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "wallet download did not match its pinned checksum ({actual}); nothing installed"
        ))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Blocking: download, verify, install into the shared plugin store.
pub fn install() -> Result<(), String> {
    let options = PluginInstallOptions::from_env().map_err(|e| format!("{e:#}"))?;
    let triple = options.target.triple().to_string();
    let expected = pin(&triple).ok_or_else(|| format!("no wallet build for {triple}"))?;
    let response = ureq::get(&archive_url(&triple))
        .timeout(std::time::Duration::from_secs(300))
        .call()
        .map_err(|e| format!("could not download the wallet: {e}"))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_ARCHIVE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("could not download the wallet: {e}"))?;
    if bytes.len() as u64 > MAX_ARCHIVE {
        return Err("wallet download is too large; nothing installed".into());
    }
    verify(&bytes, expected)?;
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let path = dir.path().join(archive_name(&triple));
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    install_plugin_archive(
        NAME,
        VERSION,
        &path,
        &options,
        &mut |_: PluginProgressEvent| {},
    )
    .map_err(|e| format!("could not install the wallet: {e:#}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_target_is_pinned_to_a_sha256() {
        for (triple, sha) in PINS {
            assert_eq!(sha.len(), 64, "{triple}");
            assert!(sha
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        }
        assert!(pin("aarch64-apple-darwin").is_some());
        assert!(pin("riscv64-unknown-linux-gnu").is_none());
    }

    #[test]
    fn release_urls_match_the_published_asset_names() {
        assert_eq!(
            archive_url("aarch64-apple-darwin"),
            "https://github.com/Mesh-LLM/lexe-wallet/releases/download/v0.1.0/lexe-wallet-v0.1.0-aarch64-apple-darwin.tar.gz"
        );
        assert!(archive_name("x86_64-pc-windows-msvc").ends_with(".zip"));
    }

    #[test]
    fn a_tampered_archive_is_refused() {
        let good = hex(&Sha256::digest(b"archive"));
        assert!(verify(b"archive", &good).is_ok());
        assert!(verify(b"tampered", &good).is_err());
    }

    /// Real download + checksum + install into a throwaway store.
    /// `cargo test --lib wallet_setup -- --ignored`
    #[test]
    #[ignore]
    fn live_install_into_a_temp_store() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("MESH_LLM_PLUGIN_DIR", dir.path());
        assert!(!installed());
        install().unwrap();
        assert!(installed());
    }
}
