//! Real tray lifecycle and reset against an isolated, ledger-only engine.
//! The engine runs as a child of this binary (`--engine`), exactly as in the app.
use mesh_tray::{lifecycle, payment_reset};

use mesh_tray::payments::{Client, Command, Mode, Policy, PolicyStatus, Pricing};
use mesh_tray::settings::{Connection, Settings};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

fn port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == [lifecycle::ENGINE_ARG] {
        let result = lifecycle::run_engine_process();
        if let Err(e) = &result {
            eprintln!("engine: {e}");
        }
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    if let [a, b, c, name] = args.as_slice() {
        if (a.as_str(), b.as_str(), c.as_str()) == ("--log-format", "json", "--plugin") {
            let result = tokio::runtime::Runtime::new().unwrap().block_on(
                mesh_llm_host_runtime::plugin::run_plugin_process(name.clone(), Vec::new()),
            );
            std::process::exit(if result.is_ok() { 0 } else { 1 });
        }
    }
    // Before starting any threads. No owner keystore exists in this fresh HOME,
    // so runtime owner resolution never enters the native Keychain load path.
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", home.path());
    std::env::set_var("MESH_LLM_PLUGIN_DIR", home.path().join("plugins"));
    std::env::remove_var("MESH_LLM_OWNER_PASSPHRASE");
    std::env::remove_var("MESH_LLM_EPHEMERAL_KEY");
    let profile = home.path().join(".mesh-llm");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::create_dir_all(home.path().join(".mesh-app")).unwrap();
    lifecycle::Engine::verify_restart_safe().unwrap();
    std::fs::write(profile.join("config.toml"), "version = 1\n").unwrap();
    let model = std::env::var("MESH_TRAY_RESTART_MODEL").ok();
    let settings = Settings {
        share_compute: model.is_some(),
        connection: Connection::Automatic,
        api_port: port(),
        console_port: port(),
    };
    // Synthetic preservation fixture, not a provisioned wallet or spendable funds.
    let wallet = profile.join("payments/wallet-provider.json");
    std::fs::create_dir_all(wallet.parent().unwrap()).unwrap();
    let wallet_pin = br#"{"plugin":"wallet-lexe","wallet_id":"preserve-test-wallet","provider":"lexe","network":"mainnet"}"#;
    std::fs::write(&wallet, wallet_pin).unwrap();
    let seed = profile.join("payments/lexe/seedphrase.txt");
    std::fs::create_dir_all(seed.parent().unwrap()).unwrap();
    let seed_fixture = b"synthetic preservation fixture - not a valid wallet seed";
    std::fs::write(&seed, seed_fixture).unwrap();
    for cycle in 0..3 {
        let mut engine = lifecycle::Engine::start(lifecycle::EngineSpec {
            settings: settings.clone(),
            profile: profile.clone(),
            model: model.clone(),
            share_compute: settings.share_compute,
            isolated_network: true,
        })
        .expect("start tray engine");
        assert_ne!(
            engine.id(),
            std::process::id(),
            "engine must be a child process"
        );
        let client = Client::new(settings.console_port, engine.id());
        let deadline = Instant::now() + Duration::from_secs(60);
        let policy = loop {
            if let Ok(policy) = client.execute::<PolicyStatus>(&Command::Policy { value: None }) {
                break policy;
            }
            assert!(
                Instant::now() < deadline,
                "payment API unavailable in cycle {cycle}"
            );
            assert!(engine.try_wait().unwrap().is_none(), "engine exited");
            std::thread::sleep(Duration::from_millis(100));
        };
        assert_eq!(policy.mode, Mode::FreeOnly);
        // No installed wallet provider: policy stays usable, but a wallet read
        // must fail rather than provision a replacement for the preserved pin.
        assert!(client
            .execute::<serde_json::Value>(&Command::Balance)
            .is_err());
        assert_eq!(std::fs::read(&wallet).unwrap(), wallet_pin);
        assert_eq!(std::fs::read(&seed).unwrap(), seed_fixture);
        let prices: BTreeMap<String, Pricing> = client.execute(&Command::Pricing).unwrap();
        assert!(prices.is_empty());
        assert!(
            profile.join("payments/payments.sqlite3").is_file(),
            "wrong payment profile"
        );
        let _: serde_json::Value = client
            .execute(&Command::Policy {
                value: Some(Policy {
                    mode: Mode::Automatic,
                    daily_budget_msat: Some(1000),
                }),
            })
            .unwrap();
        let _: serde_json::Value = client
            .execute(&Command::SetPricing {
                model: "reset-regression".into(),
                value: Some(Pricing {
                    input_msat_per_million: 1000,
                    output_msat_per_million: 1000,
                    minimum_invoice_msat: 1000,
                }),
            })
            .unwrap();
        payment_reset::disable(&client).expect("tray reset payments");
        if model.is_some() {
            println!(
                "embedded_restart: serving cycle {cycle} api={} console={}",
                settings.api_port, settings.console_port
            );
            std::thread::sleep(Duration::from_secs(20));
        }
        let held = std::fs::File::open(profile.join("payments/service.lock")).unwrap();
        assert!(
            fs2::FileExt::try_lock_exclusive(&held).is_err(),
            "running engine must hold service.lock (otherwise the release check proves nothing)"
        );
        drop(held);
        lifecycle::request_stop(&mut engine).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(result) = engine.try_wait().unwrap() {
                assert_eq!(result, "stopped");
                break;
            }
            assert!(Instant::now() < deadline, "shutdown timed out");
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(
            std::fs::read_dir(&profile).unwrap().all(|entry| {
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".mesh-config-snapshot-")
            }),
            "normal shutdown left a config snapshot"
        );
        // The bug class this design removes: a stopped engine still holding
        // the payments lock, so the next start in the same app cannot open it.
        let lock = std::fs::File::open(profile.join("payments/service.lock")).unwrap();
        fs2::FileExt::try_lock_exclusive(&lock).expect("stopped engine still holds service.lock");
        fs2::FileExt::unlock(&lock).unwrap();
        mesh_tray::private_reset::retire(&profile, &home.path().join(".mesh-app")).unwrap();
        mesh_tray::private_reset::finish(&home.path().join(".mesh-app")).unwrap();
        assert_eq!(std::fs::read(&wallet).unwrap(), wallet_pin);
        assert_eq!(std::fs::read(&seed).unwrap(), seed_fixture);
        println!("embedded_restart: reset/stop cycle {cycle} passed");
    }
}
