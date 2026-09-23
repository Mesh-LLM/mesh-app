//! Payments submenu: a thin controller over Mesh's local wallet API.
//!
//! Mesh owns the wallet, ledger, prices, budgets and settlement. The menu is
//! three fixed items; the only thing that ever changes is the submenu title,
//! which shows the last good balance once a wallet exists. Every form reads
//! Mesh fresh when clicked, so there is no menu state to go stale.
use crate::native::{self as ui, InvoiceAction};
use mesh_tray::payments::{
    self, format_sats, sats_to_msat, Balance, Client, Command, FundingInvoice, Mode, Policy,
    PolicyStatus, Pricing, Receipt, Transaction,
};
use muda::{MenuItem, Submenu};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

const REFRESH: Duration = Duration::from_secs(20);
/// Mesh's wallet side can lag its status endpoint; retry quickly for a while.
const STARTUP_GRACE: Duration = Duration::from_secs(30);
const STARTUP_RETRY: Duration = Duration::from_secs(2);
/// Minimum invoice used when a price is first set; Mesh bills at least this.
const DEFAULT_MINIMUM_MSAT: u64 = 10_000;

enum Job {
    Balance,
    ReadPolicy,
    ReadPricing(String),
    Run(Command),
    Invoice(Option<u64>),
    Receipt(String),
}

enum Reply {
    Balance(Result<Balance, payments::Error>),
    Policy(Result<PolicyStatus, payments::Error>),
    Pricing(String, Result<BTreeMap<String, Pricing>, payments::Error>),
    Done(Result<serde_json::Value, payments::Error>),
    Invoice(Result<FundingInvoice, payments::Error>),
    Receipt(Result<Receipt, payments::Error>),
}

fn describe(error: &payments::Error) -> String {
    use payments::Error::*;
    match error {
        InvalidInput(m) => (*m).into(),
        RuntimeChanged => "Mesh restarted; try again".into(),
        Unavailable => "not available in this Mesh".into(),
        Rejected(code) => format!("Mesh refused ({code})"),
        Transport => "Mesh not reachable".into(),
        InvalidResponse => "unexpected answer from Mesh".into(),
    }
}

/// Submenu title: the balance only when a wallet exists and has been read.
pub fn title(wallet: bool, balance: Option<u64>) -> String {
    match (wallet, balance) {
        (true, Some(msat)) => format!("Payments · {}", format_sats(msat)),
        _ => "Payments".into(),
    }
}

pub struct Payments {
    profile: Option<PathBuf>,
    menu: Option<Submenu>,
    shown: String,
    /// Last good balance; a failed read never clears it.
    balance: Option<u64>,
    ready_since: Option<Instant>,
    next_refresh: Instant,
    refreshing: bool,
    jobs: Sender<(u16, u32, Job)>,
    replies: Receiver<Reply>,
    last_invoice: Option<(String, String, Vec<u8>, String)>,
}

impl Payments {
    pub fn new(profile: Option<PathBuf>) -> Self {
        let (jobs, work) = mpsc::channel::<(u16, u32, Job)>();
        let (reply, replies) = mpsc::channel();
        std::thread::Builder::new()
            .name("tray-payments".into())
            .spawn(move || {
                while let Ok((port, pid, job)) = work.recv() {
                    if reply.send(run(Client::new(port, pid), job)).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn payments worker");
        Self {
            profile,
            menu: None,
            shown: String::new(),
            balance: None,
            ready_since: None,
            next_refresh: Instant::now(),
            refreshing: false,
            jobs,
            replies,
            last_invoice: None,
        }
    }

    /// Side-effect free file check; reading the balance of a missing wallet
    /// must never be what creates one.
    fn wallet(&self) -> bool {
        self.profile.as_deref().is_some_and(payments::wallet_exists)
    }

    /// Main thread only, called once from `App::build`.
    pub fn submenu(&mut self) -> Result<Submenu, String> {
        self.shown = title(self.wallet(), self.balance);
        let menu = Submenu::new(&self.shown, true);
        menu.append_items(&[
            &MenuItem::with_id("pay:pay", "Pay…", true, None),
            &MenuItem::with_id("pay:earn", "Get paid…", true, None),
            &MenuItem::with_id("pay:fund", "Add funds…", true, None),
        ])
        .map_err(|e| e.to_string())?;
        self.menu = Some(menu.clone());
        Ok(menu)
    }

    /// `target` is (console port, runtime pid) only while our own runtime is ready.
    pub fn tick(&mut self, target: Option<(u16, u32)>) {
        let now = Instant::now();
        self.set_ready(target.is_some(), now);
        while let Ok(reply) = self.replies.try_recv() {
            self.handle(reply, target);
        }
        if let Some(t) = target {
            if !self.refreshing && now >= self.next_refresh && self.wallet() {
                self.refreshing = self.jobs.send((t.0, t.1, Job::Balance)).is_ok();
                self.next_refresh = now + REFRESH;
            }
        }
        let text = title(self.wallet(), self.balance);
        if text != self.shown {
            if let Some(menu) = &self.menu {
                menu.set_text(&text);
            }
            self.shown = text;
        }
    }

    /// Becoming ready reads the balance at once instead of on the timer.
    fn set_ready(&mut self, ready: bool, now: Instant) {
        if ready == self.ready_since.is_some() {
            return;
        }
        self.ready_since = ready.then_some(now);
        self.next_refresh = now;
    }

    fn send(&mut self, (port, pid): (u16, u32), job: Job) {
        let _ = self.jobs.send((port, pid, job));
    }

    fn handle(&mut self, reply: Reply, target: Option<(u16, u32)>) {
        match reply {
            Reply::Balance(result) => {
                self.refreshing = false;
                match result {
                    Ok(b) => self.balance = Some(b.spendable_msat),
                    Err(_) => {
                        let now = Instant::now();
                        if self
                            .ready_since
                            .is_some_and(|since| now.duration_since(since) < STARTUP_GRACE)
                        {
                            self.next_refresh = now + STARTUP_RETRY;
                        }
                    }
                }
            }
            Reply::Policy(Ok(status)) => {
                if let Some(t) = target {
                    self.edit_pay(t, &status);
                }
            }
            Reply::Policy(Err(e)) => {
                ui::notice("Could not read your payment setting", &describe(&e))
            }
            Reply::Pricing(model, Ok(prices)) => {
                if let Some(t) = target {
                    let current = prices.get(&model).cloned();
                    self.edit_earn(t, model, current);
                }
            }
            Reply::Pricing(_, Err(e)) => ui::notice("Could not read your price", &describe(&e)),
            Reply::Done(Ok(_)) => self.next_refresh = Instant::now(),
            Reply::Done(Err(e)) => ui::notice(
                "Mesh did not save that",
                &format!("{}. Nothing was changed.", describe(&e)),
            ),
            Reply::Invoice(Ok(inv)) => {
                self.next_refresh = Instant::now();
                let detail = invoice_detail(self.balance, &inv, now_ms());
                match crate::qr::png(&crate::qr::lightning_uri(&inv.bolt11), 6) {
                    Ok(png) => {
                        self.last_invoice =
                            Some((inv.bolt11.clone(), detail, png, inv.payment_hash.clone()));
                        self.show_invoice(target);
                    }
                    Err(e) => ui::notice("Could not draw the QR code", &e),
                }
            }
            Reply::Invoice(Err(e)) => ui::notice("Could not create an invoice", &describe(&e)),
            Reply::Receipt(result) => {
                let text = match result {
                    Ok(Receipt::Received(msat)) => format!("Received {}.", format_sats(msat)),
                    Ok(Receipt::Pending) => "Payment is on its way but not settled yet.".into(),
                    Ok(Receipt::Failed) => "The wallet reports this payment failed.".into(),
                    Ok(Receipt::Unknown) => "No payment for this invoice yet.".into(),
                    Err(e) => format!("Could not check: {}", describe(&e)),
                };
                self.next_refresh = Instant::now();
                ui::notice("Payment status", &text);
                if !text.starts_with("Received") {
                    self.show_invoice(target);
                }
            }
        }
    }

    fn show_invoice(&mut self, target: Option<(u16, u32)>) {
        let Some((bolt11, detail, png, hash)) = self.last_invoice.clone() else {
            return;
        };
        loop {
            match ui::invoice(&detail, &bolt11, &png) {
                InvoiceAction::Copy => {
                    if let Err(e) = ui::copy_text(&bolt11) {
                        ui::notice("Could not copy", &e);
                    }
                }
                InvoiceAction::Check => {
                    if let Some(t) = target {
                        self.send(t, Job::Receipt(hash));
                    }
                    return;
                }
                InvoiceAction::Done => {
                    self.last_invoice = None;
                    return;
                }
            }
        }
    }

    /// Returns true when `id` belonged to Payments. Every item reads Mesh now.
    pub fn click(&mut self, id: &str, target: Option<(u16, u32)>, models: &[String]) -> bool {
        if !id.starts_with("pay:") {
            return false;
        }
        let Some(t) = target else {
            ui::notice("Mesh is still starting", "Try again in a moment.");
            return true;
        };
        match id {
            "pay:pay" => self.send(t, Job::ReadPolicy),
            "pay:earn" => match models.first() {
                Some(model) => self.send(t, Job::ReadPricing(model.clone())),
                None => ui::notice(
                    "Not serving a model yet",
                    "Get paid sets a price for the model this node serves. Try again once a model is loaded.",
                ),
            },
            "pay:fund" => {
                let Some(text) = ui::fund_amount() else {
                    return true;
                };
                match fund_amount(&text) {
                    Ok(amount) => self.send(t, Job::Invoice(amount)),
                    Err(e) => return invalid(&e),
                }
            }
            _ => {}
        }
        true
    }

    fn edit_pay(&mut self, t: (u16, u32), p: &PolicyStatus) {
        let (checked, budget, usage) = pay_form(p);
        let Some((on, text)) = ui::toggle_amount(
            "Pay",
            &format!("When on, Mesh may pay other nodes for models it can't use for free, up to this limit per UTC day. Turning it off stops new paid requests.\n\n{usage}"),
            "Pay for models",
            checked,
            "Daily limit (sats)",
            &budget,
        ) else {
            return;
        };
        match pay_command(on, &text) {
            Ok(command) => self.send(t, Job::Run(command)),
            Err(e) => {
                invalid(&e);
            }
        }
    }

    fn edit_earn(&mut self, t: (u16, u32), model: String, current: Option<Pricing>) {
        let (checked, price) = earn_form(current.as_ref());
        let Some((on, text)) = ui::toggle_amount(
            "Get paid",
            &format!("When on, other nodes pay you to use {model}. Off serves it for free."),
            "Charge for this model",
            checked,
            "Price (sats per M tokens)",
            &price,
        ) else {
            return;
        };
        match earn_command(model, current.as_ref(), on, &text) {
            Ok(command) => self.send(t, Job::Run(command)),
            Err(e) => {
                invalid(&e);
            }
        }
    }
}

/// What the Pay form opens with: (ticked, limit in sats, usage line).
pub fn pay_form(p: &PolicyStatus) -> (bool, String, String) {
    (
        p.mode == Mode::Automatic,
        p.daily_budget_msat
            .map(|b| (b / 1000).to_string())
            .unwrap_or_default(),
        format!(
            "Today: {} spent · {} left.",
            format_sats(p.spent_today_msat),
            format_sats(p.remaining_daily_budget_msat)
        ),
    )
}

/// What the Get paid form opens with: (ticked, price in sats).
pub fn earn_form(current: Option<&Pricing>) -> (bool, String) {
    (
        current.is_some(),
        current
            .map(|p| (p.output_msat_per_million / 1000).to_string())
            .unwrap_or_default(),
    )
}

/// Add funds amount: blank lets the payer choose.
pub fn fund_amount(text: &str) -> Result<Option<u64>, payments::Error> {
    if text.trim().is_empty() {
        Ok(None)
    } else {
        sats_to_msat(text).map(Some)
    }
}

pub fn invoice_detail(balance: Option<u64>, inv: &FundingInvoice, now_ms: u64) -> String {
    let amount = inv
        .amount_msat
        .map(format_sats)
        .unwrap_or_else(|| "any amount (payer chooses)".into());
    let minutes = inv.expires_at_ms.saturating_sub(now_ms) / 60_000;
    let balance = balance
        .map(|b| format!("Balance: {}\n\n", format_sats(b)))
        .unwrap_or_default();
    format!("{balance}Amount: {amount}\nExpires in about {minutes} min.\n\nScan with a Lightning wallet, or copy the invoice. Your balance updates once the payment arrives.")
}

/// Pay form -> command. Off keeps the typed limit so re-enabling is one click.
pub fn pay_command(on: bool, limit: &str) -> Result<Command, payments::Error> {
    let policy = if on {
        Policy {
            mode: Mode::Automatic,
            daily_budget_msat: Some(sats_to_msat(limit)?),
        }
    } else {
        Policy {
            mode: Mode::FreeOnly,
            daily_budget_msat: sats_to_msat(limit).ok(),
        }
    };
    Ok(Command::Policy {
        value: Some(policy),
    })
}

/// Get paid form -> command. Off removes the price (serve free, never 0);
/// on applies one price to input and output and keeps the existing minimum.
pub fn earn_command(
    model: String,
    current: Option<&Pricing>,
    on: bool,
    price: &str,
) -> Result<Command, payments::Error> {
    if !on {
        return Ok(Command::SetPricing { model, value: None });
    }
    let msat = sats_to_msat(price)?;
    Ok(Command::SetPricing {
        model,
        value: Some(Pricing {
            input_msat_per_million: msat,
            output_msat_per_million: msat,
            minimum_invoice_msat: current
                .map(|p| p.minimum_invoice_msat)
                .unwrap_or(DEFAULT_MINIMUM_MSAT),
        }),
    })
}

fn run(client: Client, job: Job) -> Reply {
    match job {
        Job::Balance => Reply::Balance(client.execute(&Command::Balance)),
        Job::ReadPolicy => Reply::Policy(client.execute(&Command::Policy { value: None })),
        Job::ReadPricing(model) => Reply::Pricing(model, client.execute(&Command::Pricing)),
        Job::Run(command) => Reply::Done(client.execute(&command)),
        Job::Invoice(amount_msat) => Reply::Invoice(client.execute(&Command::Fund { amount_msat })),
        Job::Receipt(hash) => Reply::Receipt(
            client
                .execute::<Vec<Transaction>>(&Command::Transactions { limit: 200 })
                .map(|txs| payments::receipt(&txs, &hash)),
        ),
    }
}

fn invalid(error: &payments::Error) -> bool {
    ui::notice("Check the amount", &describe(error));
    true
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(c: &Command) -> serde_json::Value {
        serde_json::to_value(c).unwrap()
    }

    #[test]
    fn pay_on_sets_automatic_with_limit_in_msat() {
        let c = pay_command(true, "1000").unwrap();
        assert_eq!(
            json(&c),
            serde_json::json!({"command":"policy","value":{"mode":"automatic","daily_budget_msat":1_000_000}})
        );
    }

    #[test]
    fn pay_off_is_free_only_and_keeps_the_limit() {
        let c = pay_command(false, "1000").unwrap();
        assert_eq!(json(&c)["value"]["mode"], "free_only");
        assert_eq!(json(&c)["value"]["daily_budget_msat"], 1_000_000);
        // Blank limit when turning off is fine; nothing to keep.
        let c = pay_command(false, "").unwrap();
        assert!(json(&c)["value"]["daily_budget_msat"].is_null());
    }

    #[test]
    fn pay_on_needs_a_valid_limit() {
        assert!(pay_command(true, "").is_err());
        assert!(pay_command(true, "abc").is_err());
    }

    #[test]
    fn earn_off_removes_the_price_instead_of_zero() {
        let c = earn_command("m".into(), None, false, "500").unwrap();
        assert_eq!(
            json(&c),
            serde_json::json!({"command":"set_pricing","model":"m","value":null})
        );
    }

    #[test]
    fn earn_on_uses_one_price_for_both_and_keeps_minimum() {
        let existing = Pricing {
            input_msat_per_million: 1,
            output_msat_per_million: 2,
            minimum_invoice_msat: 42_000,
        };
        let c = earn_command("m".into(), Some(&existing), true, "100").unwrap();
        let v = json(&c)["value"].clone();
        assert_eq!(v["input_msat_per_million"], 100_000);
        assert_eq!(v["output_msat_per_million"], 100_000);
        assert_eq!(v["minimum_invoice_msat"], 42_000);
        let c = earn_command("m".into(), None, true, "100").unwrap();
        assert_eq!(
            json(&c)["value"]["minimum_invoice_msat"],
            DEFAULT_MINIMUM_MSAT
        );
        assert!(earn_command("m".into(), None, true, "").is_err());
    }

    fn status(mode: Mode, budget: Option<u64>) -> PolicyStatus {
        PolicyStatus {
            mode,
            daily_budget_msat: budget,
            spent_today_msat: 250_000,
            reserved_msat: 0,
            remaining_daily_budget_msat: 750_000,
        }
    }

    #[test]
    fn pay_form_prefills_from_mesh() {
        let (on, limit, usage) = pay_form(&status(Mode::Automatic, Some(1_000_000)));
        assert!(on);
        assert_eq!(limit, "1000");
        assert_eq!(usage, "Today: 250 sats spent · 750 sats left.");
        let (on, limit, _) = pay_form(&status(Mode::FreeOnly, None));
        assert!(!on);
        assert_eq!(limit, "");
    }

    #[test]
    fn saving_the_pay_form_unchanged_keeps_the_setting() {
        for s in [
            status(Mode::Automatic, Some(1_000_000)),
            status(Mode::FreeOnly, Some(5_000_000)),
            status(Mode::FreeOnly, None),
        ] {
            let (on, limit, _) = pay_form(&s);
            let Command::Policy { value: Some(p) } = pay_command(on, &limit).unwrap() else {
                panic!("not a policy command");
            };
            assert_eq!(
                (p.mode, p.daily_budget_msat),
                (s.mode.clone(), s.daily_budget_msat)
            );
        }
    }

    #[test]
    fn saving_the_earn_form_unchanged_keeps_the_price() {
        let existing = Pricing {
            input_msat_per_million: 100_000,
            output_msat_per_million: 100_000,
            minimum_invoice_msat: 42_000,
        };
        let (on, price) = earn_form(Some(&existing));
        assert!(on);
        let Command::SetPricing { value, .. } =
            earn_command("m".into(), Some(&existing), on, &price).unwrap()
        else {
            panic!("not a pricing command");
        };
        assert_eq!(value, Some(existing));
        assert_eq!(earn_form(None), (false, String::new()));
    }

    #[test]
    fn fund_amount_blank_lets_payer_choose() {
        assert_eq!(fund_amount("").unwrap(), None);
        assert_eq!(fund_amount("  ").unwrap(), None);
        assert_eq!(fund_amount("5000").unwrap(), Some(5_000_000));
        assert!(fund_amount("-1").is_err());
    }

    #[test]
    fn invoice_detail_shows_balance_amount_and_expiry() {
        let inv = FundingInvoice {
            bolt11: "lnbc1".into(),
            payment_hash: "h".into(),
            payee: "p".into(),
            amount_msat: None,
            expires_at_ms: 10 * 60_000,
        };
        let d = invoice_detail(Some(1_234_000), &inv, 0);
        assert!(d.starts_with("Balance: 1,234 sats"));
        assert!(d.contains("any amount (payer chooses)"));
        assert!(d.contains("about 10 min"));
        assert!(!invoice_detail(None, &inv, 0).contains("Balance"));
    }

    #[test]
    fn title_shows_balance_only_with_a_wallet() {
        assert_eq!(title(false, Some(1_234_000)), "Payments");
        assert_eq!(title(true, None), "Payments");
        assert_eq!(title(true, Some(1_234_000)), "Payments · 1,234 sats");
    }

    #[test]
    fn failed_read_keeps_last_good_balance() {
        let mut p = Payments::new(None);
        p.handle(
            Reply::Balance(Ok(Balance {
                spendable_msat: 5_000,
                available_for_inference_msat: 5_000,
            })),
            None,
        );
        p.handle(Reply::Balance(Err(payments::Error::Transport)), None);
        assert_eq!(p.balance, Some(5_000));
    }

    #[test]
    fn becoming_ready_reads_now_and_startup_failures_retry_fast() {
        let mut p = Payments::new(None);
        p.next_refresh = Instant::now() + Duration::from_secs(3600);
        p.set_ready(true, Instant::now());
        assert!(p.next_refresh <= Instant::now());
        p.handle(Reply::Balance(Err(payments::Error::Transport)), None);
        assert!(p.next_refresh <= Instant::now() + STARTUP_RETRY);
        assert_eq!(p.balance, None);
    }
}
