//! Real engine, real Lexe wallet, throwaway profile. Opt-in; touches no funds.
//!
//!   MESH_TRAY_WALLET_E2E=1 cargo test --test isolated_wallet
//!
//! Starts the embedded engine client-only (no model) in a temp config dir, so
//! its payments ledger and wallet live in that dir and are deleted afterwards.
//! No mesh join, no publishing, no relays. Drives every command the tray's
//! wallet client sends. `Fund` only creates an invoice; nothing is paid.
//! Each run provisions a fresh, empty mainnet Lexe wallet over the network.
use mesh_llm_sdk::{client, serve};
use mesh_tray::payments::*;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

fn main() {
    // The engine launches its wallet as `current_exe --log-format json --plugin
    // wallet-lexe`; current_exe is this test binary, so serve that here.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [a, b, c, name] = args.as_slice() {
        if (a.as_str(), b.as_str(), c.as_str()) == ("--log-format", "json", "--plugin") {
            let result = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(mesh_llm_host_runtime::plugin::run_plugin_process(
                    name.clone(),
                ));
            std::process::exit(if result.is_ok() { 0 } else { 1 });
        }
    }
    if std::env::var_os("MESH_TRAY_WALLET_E2E").is_none() {
        println!("isolated_wallet: skipped (set MESH_TRAY_WALLET_E2E=1)");
        return;
    }
    // The engine also writes node state (last-mesh, ownership, logs, plugin
    // sockets) under $HOME/.mesh-llm regardless of config_path. Point HOME at
    // a temp dir before any thread starts; the wallet plugin inherits it.
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", home.path());
    round_trip();
    drop(home);
    println!("isolated_wallet: ok");
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn step<T>(label: &str, t: Instant, r: Result<T, Error>) -> T {
    match r {
        Ok(v) => {
            eprintln!("  {label}: ok ({:?})", t.elapsed());
            v
        }
        Err(e) => panic!("{label}: {e:?} after {:?}", t.elapsed()),
    }
}

fn round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let (api, console) = (free_port(), free_port());
    let config = serve::EmbeddedServeConfig::builder()
        .api_port(api)
        .console_port(console)
        .console_ui(true)
        .config_path(dir.path().join("config.toml"))
        .isolated_config(false)
        .auto_join(false)
        .publish(false)
        .disable_iroh_relays(true)
        .startup_timeout(Duration::from_secs(120))
        .build();
    let config = client::EmbeddedClientConfig {
        http: config.http,
        network: config.network,
        admission: config.admission,
        storage: config.storage,
        log_format: config.log_format,
        startup_timeout: config.startup_timeout,
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let t = Instant::now();
    let handle = rt.block_on(client::start(config)).expect("engine start");
    eprintln!("  engine started ({:?})", t.elapsed());

    let c = Client::new(console, std::process::id());
    let before: PolicyStatus = step(
        "read policy",
        t,
        c.execute(&Command::Policy { value: None }),
    );
    eprintln!("    default mode {:?}", before.mode);
    let _: serde_json::Value = step(
        "save policy",
        t,
        c.execute(&Command::Policy {
            value: Some(Policy {
                mode: Mode::FreeOnly,
                daily_budget_msat: Some(1_234_000),
            }),
        }),
    );
    let saved: PolicyStatus = step(
        "re-read policy",
        t,
        c.execute(&Command::Policy { value: None }),
    );
    assert_eq!(saved.mode, Mode::FreeOnly);
    assert_eq!(saved.daily_budget_msat, Some(1_234_000));

    let price = Pricing {
        input_msat_per_million: 100_000,
        output_msat_per_million: 100_000,
        minimum_invoice_msat: 10_000,
    };
    let _: serde_json::Value = step(
        "save price",
        t,
        c.execute(&Command::SetPricing {
            model: "isolated-check".into(),
            value: Some(price.clone()),
        }),
    );
    let prices: BTreeMap<String, Pricing> = step("read pricing", t, c.execute(&Command::Pricing));
    assert_eq!(prices.get("isolated-check"), Some(&price));

    let balance: Balance = step("balance", t, c.execute(&Command::Balance));
    assert_eq!(balance.spendable_msat, 0, "fresh wallet must be empty");

    let invoice: FundingInvoice = step(
        "fund invoice",
        t,
        c.execute(&Command::Fund {
            amount_msat: Some(10_000),
        }),
    );
    assert!(invoice.bolt11.starts_with("lnbc"), "mainnet bolt11");
    assert_eq!(invoice.amount_msat, Some(10_000));
    assert!(!invoice.payment_hash.is_empty());

    let txs: Vec<Transaction> = step(
        "transactions",
        t,
        c.execute(&Command::Transactions { limit: 50 }),
    );
    assert!(txs.iter().all(|tx| tx.status != PaymentStatus::Succeeded));

    rt.block_on(handle.stop()).expect("engine stop");
    eprintln!("  engine stopped ({:?})", t.elapsed());
}
