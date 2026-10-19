//! Read-only check of the operator's real profile and installed wallet.
//! Opt-in; spends nothing and changes no settings.
//!
//!   MESH_TRAY_PROFILE_E2E=1 cargo test --test profile_wallet
//!
//! Starts the engine client-only against `~/.mesh-llm` (no mesh join, no
//! publishing), then reads policy, prices and balance the way the tray does.
use mesh_llm_sdk::MeshNode;
use mesh_tray::payments::*;
use std::collections::BTreeMap;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [a, b, c, name] = args.as_slice() {
        if (a.as_str(), b.as_str(), c.as_str()) == ("--log-format", "json", "--plugin") {
            let result = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(mesh_llm_sdk::plugin::run_plugin_process(
                    name.clone(),
                    Vec::new(),
                ));
            std::process::exit(if result.is_ok() { 0 } else { 1 });
        }
    }
    if std::env::var_os("MESH_TRAY_PROFILE_E2E").is_none() {
        println!("profile_wallet: skipped (set MESH_TRAY_PROFILE_E2E=1)");
        return;
    }
    let profile = mesh_tray::settings::mesh_profile().unwrap();
    let port = |p: u16| p;
    let (api, console) = (port(19447), port(13232));
    let node = MeshNode::builder()
        .client()
        .api_port(api)
        .console_port(console)
        .console_ui(true)
        .config_path(profile.join("config.toml"))
        .auto_join(false)
        .publish(false)
        .startup_timeout(Duration::from_secs(120));
    let rt = tokio::runtime::Runtime::new().unwrap();
    let handle = rt.block_on(node.start()).expect("engine start");
    let c = Client::new(console, std::process::id());
    let policy = c.execute::<PolicyStatus>(&Command::Policy { value: None });
    eprintln!("  policy: {policy:?}");
    let prices = c.execute::<BTreeMap<String, Pricing>>(&Command::Pricing);
    eprintln!("  prices: {prices:?}");
    let balance = c.execute::<Balance>(&Command::Balance);
    eprintln!("  balance: {balance:?}");
    rt.block_on(handle.stop()).expect("engine stop");
    let ok = policy.is_ok() && prices.is_ok() && balance.is_ok();
    assert!(ok, "real profile payments must answer");
    println!("profile_wallet: ok");
}
