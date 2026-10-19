//! Payments submenu: a thin controller over Mesh's local wallet API.
//!
//! Mesh owns the wallet, ledger, prices, budgets and settlement. The menu is
//! three fixed actions, plus a disabled status item when balance reads fail.
//! The title shows a balance only after a successful read with a wallet present.
//! Every form reads Mesh fresh when clicked.
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

/// Balance poll while Mesh is ready. Any action refreshes at once; a failed
/// read just waits for the next tick and keeps the last good balance.
const REFRESH: Duration = Duration::from_secs(10);

enum Job {
    Balance,
    Reset,
    /// Private mesh: turn spending off and remove every price.
    PrivateOff(u32),
    ReadPolicy,
    ReadPricing(String),
    Run(Command),
    Invoice(Option<u64>),
    Receipt(String),
}

enum Reply {
    Reset(Result<(), String>),
    PrivateOff(u32, Result<(), String>),
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
        Rejected(_, Some(why)) => format!("Mesh refused: {why}"),
        Rejected(code, None) => format!("Mesh refused ({code})"),
        Transport => "Mesh not reachable".into(),
        InvalidResponse => "unexpected answer from Mesh".into(),
    }
}

/// Submenu title: the balance only when a wallet exists and has been read.
pub fn title(wallet: bool, balance: Option<u64>, failed: bool) -> String {
    // Keep the last good value internally, but do not present a stale balance
    // as current after a failed refresh. Unknown is distinct from known zero.
    if failed {
        return "Payments".into();
    }
    match (wallet, balance) {
        (true, Some(msat)) => format!("Payments · {}", format_sats(msat)),
        _ => "Payments".into(),
    }
}

/// (clicked item, runtime pid, install result).
type Setup = (String, u32, Receiver<Result<(), String>>);

pub struct Payments {
    profile: Option<PathBuf>,
    menu: Option<Submenu>,
    shown: String,
    unavailable: Option<MenuItem>,
    /// Last good balance; a failed read never clears it.
    balance: Option<u64>,
    balance_failed: bool,
    next_refresh: Instant,
    refreshing: bool,
    jobs: Sender<(u16, u32, Job)>,
    replies: Receiver<Reply>,
    reset_result: Option<Result<(), String>>,
    /// Runtime pid whose payments were verified off for private mode.
    private_off: Option<u32>,
    /// Wallet plugin install in flight: (clicked item, runtime pid, result).
    setup: Option<Setup>,
    /// Installed; the engine must restart to load it, then `.0` reopens.
    replay: Option<(String, u32)>,
    restart_requested: bool,
    private_off_pending: bool,
    next_private_off: Instant,
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
            unavailable: None,
            balance: None,
            balance_failed: false,
            next_refresh: Instant::now(),
            refreshing: false,
            jobs,
            replies,
            last_invoice: None,
            reset_result: None,
            private_off: None,
            setup: None,
            replay: None,
            restart_requested: false,
            private_off_pending: false,
            next_private_off: Instant::now(),
        }
    }

    pub fn begin_reset(&mut self, target: (u16, u32)) -> Result<(), String> {
        self.reset_result = None;
        self.jobs
            .send((target.0, target.1, Job::Reset))
            .map_err(|_| "Payments worker is unavailable".into())
    }

    pub fn take_reset_result(&mut self) -> Option<Result<(), String>> {
        self.reset_result.take()
    }

    /// Side-effect free file check; reading the balance of a missing wallet
    /// must never be what creates one.
    fn wallet(&self) -> bool {
        self.profile.as_deref().is_some_and(payments::wallet_exists)
    }

    /// Main thread only, called once from `App::build`.
    pub fn submenu(&mut self) -> Result<Submenu, String> {
        self.shown = title(self.wallet(), self.balance, self.balance_failed);
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
    /// `public` is false on a private mesh: no balance polling, and the engine
    /// is told once per runtime to stop paying and charging (retried until
    /// verified). Wallet files are never touched.
    pub fn tick(&mut self, target: Option<(u16, u32)>, public: bool) {
        let now = Instant::now();
        while let Ok(reply) = self.replies.try_recv() {
            self.handle(reply, target);
        }
        self.poll_setup();
        if public {
            self.private_off = None;
        } else if let Some(t) = target {
            if self.private_off != Some(t.1)
                && !self.private_off_pending
                && now >= self.next_private_off
            {
                self.private_off_pending = self.jobs.send((t.0, t.1, Job::PrivateOff(t.1))).is_ok();
                self.next_private_off = now + REFRESH;
            }
        }
        if let Some(t) = target.filter(|_| public) {
            if !self.refreshing && now >= self.next_refresh && self.wallet() {
                self.refreshing = self.jobs.send((t.0, t.1, Job::Balance)).is_ok();
                self.next_refresh = now + REFRESH;
            }
        }
        if let Some(menu) = self.menu.as_ref().filter(|_| !crate::native::menu_open()) {
            if self.balance_failed && self.unavailable.is_none() {
                let item = MenuItem::new("Balance unavailable", false, None);
                if menu.append(&item).is_ok() {
                    self.unavailable = Some(item);
                }
            } else if !self.balance_failed {
                if let Some(item) = self.unavailable.take() {
                    let _ = menu.remove(&item);
                }
            }
        }
        let text = title(self.wallet(), self.balance, self.balance_failed);
        if text != self.shown && !crate::native::menu_open() {
            if let Some(menu) = &self.menu {
                menu.set_text(&text);
            }
            self.shown = text;
        }
    }

    /// Main asks after every tick: restart the engine so it loads the wallet.
    /// Last good balance (msat) unless the latest read failed.
    pub fn balance(&self) -> Option<u64> {
        self.balance
            .filter(|_| !self.balance_failed && self.wallet())
    }

    pub fn has_wallet(&self) -> bool {
        self.wallet()
    }

    pub fn take_restart_request(&mut self) -> bool {
        std::mem::take(&mut self.restart_requested)
    }

    /// The item to reopen once a restarted engine (new pid) is ready.
    pub fn take_replay(&mut self, target: Option<(u16, u32)>) -> Option<String> {
        let (_, old_pid) = self.replay.as_ref()?;
        let (_, pid) = target?;
        if pid == *old_pid {
            return None;
        }
        self.replay.take().map(|(id, _)| id)
    }

    fn poll_setup(&mut self) {
        let Some((_, _, result)) = &self.setup else {
            return;
        };
        let outcome = match result.try_recv() {
            Ok(outcome) => outcome,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("wallet setup stopped".into()),
        };
        let (id, pid, _) = self.setup.take().expect("checked above");
        match outcome {
            Ok(()) => {
                self.replay = Some((id, pid));
                self.restart_requested = true;
            }
            Err(e) => ui::notice(
                "Could not set up your wallet",
                &format!("{e}. Nothing was changed; try again."),
            ),
        }
    }

    fn send(&mut self, (port, pid): (u16, u32), job: Job) {
        let _ = self.jobs.send((port, pid, job));
    }

    fn handle(&mut self, reply: Reply, target: Option<(u16, u32)>) {
        match reply {
            Reply::Reset(result) => self.reset_result = Some(result),
            Reply::PrivateOff(pid, result) => {
                self.private_off_pending = false;
                match result {
                    Ok(()) => self.private_off = Some(pid),
                    // Logged to mesh.log; retried on the next refresh.
                    Err(e) => eprintln!("private mode: could not turn payments off: {e}"),
                }
            }
            Reply::Balance(result) => {
                self.refreshing = false;
                self.balance_failed = result.is_err();
                if let Ok(b) = result {
                    self.balance = Some(b.spendable_msat);
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
        if self.setup.is_some() || self.replay.is_some() {
            ui::notice(
                "Setting up your wallet",
                "This opens by itself when it is ready.",
            );
            return true;
        }
        if !mesh_tray::wallet_setup::installed() {
            let (done, result) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = done.send(mesh_tray::wallet_setup::install());
            });
            self.setup = Some((id.to_string(), t.1, result));
            ui::notice(
                "Setting up your wallet",
                "Paying and getting paid use a Lightning wallet. Mesh is installing one for you and will restart; this opens by itself when it is ready.",
            );
            return true;
        }
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
        let form = earn_form(current.as_ref());
        let Some((on, values)) = ui::toggle_fields(
            "Get paid",
            &format!("When on, other nodes pay you to use {model}. Off serves it for free."),
            "Charge for this model",
            form.on,
            &[
                ("Input (sats per M tokens)", &form.input),
                ("Output (sats per M tokens)", &form.output),
                ("Cached input discount (%)", &form.cache),
            ],
        ) else {
            return;
        };
        let [input, output, cache] = values;
        let form = EarnForm {
            on,
            input,
            output,
            cache,
        };
        match earn_command(model, &form) {
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

/// The Get paid form's values, as shown and as typed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EarnForm {
    pub on: bool,
    pub input: String,
    pub output: String,
    pub cache: String,
}

/// What the Get paid form opens with.
pub fn earn_form(current: Option<&Pricing>) -> EarnForm {
    current.map_or_else(EarnForm::default, |p| EarnForm {
        on: true,
        input: (p.input_msat_per_million / 1000).to_string(),
        output: (p.output_msat_per_million / 1000).to_string(),
        cache: p.cache_discount_percent.to_string(),
    })
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

/// Get paid form -> command. Off removes the price (serve free, never 0).
/// Output blank copies input; a blank discount is 0%.
pub fn earn_command(model: String, form: &EarnForm) -> Result<Command, payments::Error> {
    if !form.on {
        return Ok(Command::SetPricing { model, value: None });
    }
    let input = sats_to_msat(&form.input)?;
    let output = if form.output.trim().is_empty() {
        input
    } else {
        sats_to_msat(&form.output)?
    };
    let cache = form.cache.trim();
    let cache_discount_percent = if cache.is_empty() {
        0
    } else {
        cache
            .parse::<u8>()
            .ok()
            .filter(|p| *p <= 100)
            .ok_or(payments::Error::InvalidInput(
                "Cached input discount must be 0 to 100",
            ))?
    };
    Ok(Command::SetPricing {
        model,
        value: Some(Pricing {
            input_msat_per_million: input,
            output_msat_per_million: output,
            cache_discount_percent,
        }),
    })
}

fn run(client: Client, job: Job) -> Reply {
    match job {
        Job::Reset => Reply::Reset(crate::payment_reset::disable(&client)),
        Job::PrivateOff(pid) => Reply::PrivateOff(pid, crate::payment_reset::disable(&client)),
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

    fn form(on: bool, input: &str, output: &str, cache: &str) -> EarnForm {
        EarnForm {
            on,
            input: input.into(),
            output: output.into(),
            cache: cache.into(),
        }
    }

    #[test]
    fn earn_off_removes_the_price_instead_of_zero() {
        let c = earn_command("m".into(), &form(false, "500", "", "")).unwrap();
        assert_eq!(
            json(&c),
            serde_json::json!({"command":"set_pricing","model":"m","value":null})
        );
    }

    #[test]
    fn earn_on_sets_input_output_and_cache_discount() {
        let c = earn_command("m".into(), &form(true, "100", "300", "50")).unwrap();
        let v = json(&c)["value"].clone();
        assert_eq!(v["input_msat_per_million"], 100_000);
        assert_eq!(v["output_msat_per_million"], 300_000);
        assert_eq!(v["cache_discount_percent"], 50);
        let c = earn_command("m".into(), &form(true, "100", "", "")).unwrap();
        let v = json(&c)["value"].clone();
        assert_eq!(
            v["output_msat_per_million"], 100_000,
            "blank output copies input"
        );
        assert!(v.get("cache_discount_percent").is_none());
        assert!(earn_command("m".into(), &form(true, "", "", "")).is_err());
        assert!(earn_command("m".into(), &form(true, "1", "1", "101")).is_err());
    }

    fn status(mode: Mode, budget: Option<u64>) -> PolicyStatus {
        PolicyStatus {
            mode,
            daily_budget_msat: budget,
            spent_today_msat: 250_000,
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
            output_msat_per_million: 250_000,
            cache_discount_percent: 42,
        };
        let f = earn_form(Some(&existing));
        assert!(f.on);
        let Command::SetPricing { value, .. } = earn_command("m".into(), &f).unwrap() else {
            panic!("not a pricing command");
        };
        assert_eq!(value, Some(existing));
        assert_eq!(earn_form(None), EarnForm::default());
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
        assert_eq!(title(false, Some(1_234_000), false), "Payments");
        assert_eq!(title(true, None, false), "Payments");
        assert_eq!(title(true, None, true), "Payments");
        assert_eq!(title(true, Some(0), true), "Payments");
        assert_eq!(title(true, Some(0), false), "Payments · 0 sats");
        assert_eq!(title(true, Some(1_234_000), false), "Payments · 1,234 sats");
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
        assert_eq!(title(true, p.balance, p.balance_failed), "Payments");
        p.handle(
            Reply::Balance(Ok(Balance {
                spendable_msat: 0,
                available_for_inference_msat: 0,
            })),
            None,
        );
        assert_eq!(
            title(true, p.balance, p.balance_failed),
            "Payments · 0 sats"
        );
    }
}
