use super::*;
use std::net::TcpListener;
use std::thread;

fn fixture(status: u16, body: &str) -> (Client, thread::JoinHandle<serde_json::Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let body = body.to_owned();
    let worker = thread::spawn(move || {
        use std::io::{BufRead, BufReader, Write};
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(&mut socket);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "POST /api/wallet HTTP/1.1\r\n");
        let mut length = 0;
        loop {
            line.clear();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    length = value.trim().parse::<usize>().unwrap();
                }
            }
        }
        let mut request = vec![0; length];
        reader.read_exact(&mut request).unwrap();
        write!(socket, "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        serde_json::from_slice(&request).unwrap()
    });
    (Client::new(port, 42), worker)
}

#[test]
fn exact_money_input_rejects_unsafe_values() {
    assert_eq!(sats_to_msat(" 12 "), Ok(12_000));
    for input in ["0", "-1", "1.5", "1e3", "+1", "", "18446744073709551615"] {
        assert!(sats_to_msat(input).is_err(), "{input}");
    }
}

#[test]
fn policy_read_pins_runtime_and_preserves_engine_usage() {
    let (client, worker) = fixture(
        200,
        r#"{"mode":"automatic","daily_budget_msat":10000,"spent_today_msat":2000,"reserved_msat":3000,"remaining_daily_budget_msat":5000}"#,
    );
    let result: PolicyStatus = client.execute(&Command::Policy { value: None }).unwrap();
    assert_eq!(result.remaining_daily_budget_msat, 5000);
    assert_eq!(result.reserved_msat, 3000);
    assert_eq!(
        worker.join().unwrap(),
        serde_json::json!({"command":"policy","value":null,"expected_pid":42})
    );
}

#[test]
fn disabling_price_removes_it_instead_of_sending_zero() {
    let command = Command::SetPricing {
        model: "canonical-model".into(),
        value: None,
    };
    assert!(validate(&command).is_ok());
    assert_eq!(
        serde_json::to_value(command).unwrap(),
        serde_json::json!({"command":"set_pricing","model":"canonical-model","value":null})
    );
}

#[test]
fn invalid_commands_fail_before_network_access() {
    let client = Client::new(0, 42);
    for command in [
        Command::Fund {
            amount_msat: Some(0),
        },
        Command::Policy {
            value: Some(Policy {
                mode: Mode::Automatic,
                daily_budget_msat: None,
            }),
        },
        Command::SetPricing {
            model: "".into(),
            value: None,
        },
        Command::SetPricing {
            model: "model".into(),
            value: Some(Pricing {
                input_msat_per_million: 0,
                output_msat_per_million: 1,
                minimum_invoice_msat: 1,
            }),
        },
    ] {
        assert!(matches!(
            client.execute::<serde_json::Value>(&command),
            Err(Error::InvalidInput(_))
        ));
    }
}

#[test]
fn runtime_mismatch_and_unavailable_are_distinct() {
    for (status, expected) in [
        (409, Error::RuntimeChanged),
        (404, Error::Unavailable),
        (400, Error::Rejected(400)),
        (302, Error::Rejected(302)),
    ] {
        let (client, worker) = fixture(status, "{}");
        assert_eq!(
            client.execute::<serde_json::Value>(&Command::Pricing),
            Err(expected)
        );
        worker.join().unwrap();
    }
}

#[test]
fn malformed_or_incomplete_response_is_not_zero_balance() {
    for body in ["broken", "{}"] {
        let (client, worker) = fixture(200, body);
        assert!(matches!(
            client.execute::<Balance>(&Command::Balance),
            Err(Error::InvalidResponse)
        ));
        worker.join().unwrap();
    }
}

#[test]
fn funding_is_an_explicit_command_with_no_spending_policy() {
    let command = Command::Fund {
        amount_msat: Some(20_000),
    };
    assert_eq!(
        serde_json::to_value(command).unwrap(),
        serde_json::json!({"command":"fund","amount_msat":20000})
    );
    // Blank amount: the payer's wallet chooses, as the PR #1926 API allows.
    assert_eq!(
        serde_json::to_value(Command::Fund { amount_msat: None }).unwrap(),
        serde_json::json!({"command":"fund","amount_msat":null})
    );
}

#[test]
fn receipt_matches_the_invoice_hash_not_balance_growth() {
    let txs: Vec<Transaction> = serde_json::from_str(
        r#"[{"id":"a","payment_hash":"other","inbound":true,"amount_msat":9000,"fee_msat":0,"status":"succeeded","created_at_ms":1,"settled_at_ms":2},
            {"id":"b","payment_hash":"mine","inbound":false,"amount_msat":5000,"fee_msat":0,"status":"succeeded","created_at_ms":1,"settled_at_ms":2},
            {"id":"c","payment_hash":"mine","inbound":true,"amount_msat":21000,"fee_msat":0,"status":"succeeded","created_at_ms":1,"settled_at_ms":2}]"#,
    )
    .unwrap();
    assert_eq!(receipt(&txs, "mine"), Receipt::Received(21_000));
    assert_eq!(receipt(&txs[..2], "mine"), Receipt::Unknown);
    assert_eq!(receipt(&[], "mine"), Receipt::Unknown);
}

#[test]
fn sats_display_groups_thousands() {
    assert_eq!(format_sats(0), "0 sats");
    assert_eq!(format_sats(1_000), "1 sat");
    assert_eq!(format_sats(1_234_999), "1,234 sats");
    assert_eq!(format_sats(1_000_000_000), "1,000,000 sats");
}

#[test]
fn existing_wallet_is_detected_without_touching_it() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!wallet_exists(dir.path()));
    std::fs::create_dir_all(dir.path().join("payments/lexe")).unwrap();
    assert!(wallet_exists(dir.path()));
    let pinned = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(pinned.path().join("payments")).unwrap();
    std::fs::write(pinned.path().join("payments/wallet-provider.json"), "{}").unwrap();
    assert!(wallet_exists(pinned.path()));
}
