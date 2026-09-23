//! Payments submenu: a thin controller over Mesh's local wallet API.
//!
//! Mesh owns the wallet, ledger, prices, budgets and settlement. This file
//! only turns clicks into `/api/wallet` commands on a worker thread and shows
//! the last answer Mesh gave. Nothing here computes or stores money state.
use crate::native::{self as ui, InvoiceAction};
use mesh_tray::payments::{
    self, format_sats, sats_to_msat, Balance, Client, Command, FundingInvoice, Mode, Policy,
    PolicyStatus, Pricing, Receipt, Transaction,
};
use muda::{MenuItem, PredefinedMenuItem, Submenu};
use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

const REFRESH: Duration = Duration::from_secs(20);

enum Job {
    Refresh { wallet: bool },
    Run(Command),
    Invoice(Option<u64>),
    Receipt(String),
}

enum Reply {
    Refreshed {
        balance: Option<Result<Balance, payments::Error>>,
        policy: Result<PolicyStatus, payments::Error>,
        pricing: Result<BTreeMap<String, Pricing>, payments::Error>,
    },
    Done(Result<serde_json::Value, payments::Error>),
    Invoice(Result<FundingInvoice, payments::Error>),
    Receipt(Result<Receipt, payments::Error>),
}

/// Last answers from Mesh. `None` = never read; errors are shown, not zeroed.
#[derive(Default)]
pub struct View {
    pub wallet: bool,
    pub balance: Option<Result<u64, String>>,
    pub policy: Option<Result<PolicyStatus, String>>,
    pub pricing: Option<Result<BTreeMap<String, Pricing>, String>>,
    pub models: Vec<String>,
}

fn describe(error: &payments::Error) -> String {
    use payments::Error::*;
    match error {
        InvalidInput(m) => (*m).into(),
        RuntimeChanged => "Mesh restarted; refreshing".into(),
        Unavailable => "not available in this Mesh".into(),
        Rejected(code) => format!("Mesh refused ({code})"),
        Transport => "Mesh not reachable".into(),
        InvalidResponse => "unexpected answer from Mesh".into(),
    }
}

/// The Payments menu text, derived from the last answers only. Pure so the
/// labels are testable without AppKit; the items themselves never change.
#[derive(Debug, PartialEq, Eq)]
pub struct Labels {
    pub title: String,
    pub balance: String,
    pub fund: &'static str,
    pub spending: String,
    pub earning: String,
    pub can_price: bool,
}

pub fn labels(view: &View) -> Labels {
    let (title, balance) = match (&view.balance, view.wallet) {
        (_, false) => ("Payments".into(), "Wallet not enabled".into()),
        (None, true) => ("Payments".into(), "Balance: checking…".into()),
        (Some(Ok(msat)), true) => (
            format!("Payments · {}", format_sats(*msat)),
            format!("Balance: {}", format_sats(*msat)),
        ),
        (Some(Err(e)), true) => ("Payments".into(), format!("Balance unavailable — {e}")),
    };
    let spending = match &view.policy {
        None => "Spending: checking…".into(),
        Some(Err(e)) => format!("Spending unavailable — {e}"),
        Some(Ok(p)) if p.mode == Mode::FreeOnly => "Spending: free only".into(),
        Some(Ok(p)) => format!(
            "Spending: on · {} left today",
            format_sats(p.remaining_daily_budget_msat)
        ),
    };
    let earning = match (view.models.first(), &view.pricing) {
        (None, _) => "Earning: no model being served".into(),
        (Some(_), None) => "Earning: checking…".into(),
        (Some(_), Some(Err(e))) => format!("Earning unavailable — {e}"),
        (Some(m), Some(Ok(prices))) => match prices.get(m) {
            None => "Earning: free".into(),
            Some(p) => format!(
                "Earning: {} in · {} out per M tokens",
                format_sats(p.input_msat_per_million),
                format_sats(p.output_msat_per_million)
            ),
        },
    };
    Labels {
        title,
        balance,
        fund: if view.wallet {
            "Add funds…"
        } else {
            "Enable wallet…"
        },
        spending,
        earning,
        can_price: !view.models.is_empty(),
    }
}

/// Built once with the tray; polling only updates text and enabled state.
struct Items {
    menu: Submenu,
    balance: MenuItem,
    fund: MenuItem,
    spending: MenuItem,
    earning: MenuItem,
    price: MenuItem,
}

pub struct Payments {
    pub view: View,
    /// Created with the tray on the main thread; AppKit menus are main-thread only.
    items: Option<Items>,
    shown: Option<Labels>,
    jobs: Sender<(u16, u32, Job)>,
    replies: Receiver<Reply>,
    busy: bool,
    next_refresh: Instant,
    last_invoice: Option<(String, String, Vec<u8>, String)>,
}

impl Payments {
    pub fn new(wallet: bool) -> Self {
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
            view: View {
                wallet,
                ..View::default()
            },
            items: None,
            shown: None,
            jobs,
            replies,
            busy: false,
            next_refresh: Instant::now(),
            last_invoice: None,
        }
    }

    /// Main thread only, called once from `App::build`.
    pub fn submenu(&mut self) -> Result<Submenu, String> {
        let l = labels(&self.view);
        let items = Items {
            menu: Submenu::new(&l.title, true),
            balance: MenuItem::with_id("pay:balance", &l.balance, false, None),
            fund: MenuItem::with_id("pay:fund", l.fund, true, None),
            spending: MenuItem::with_id("pay:spend-status", &l.spending, false, None),
            earning: MenuItem::with_id("pay:earn-status", &l.earning, false, None),
            price: MenuItem::with_id("pay:price", "Set price…", l.can_price, None),
        };
        items
            .menu
            .append_items(&[
                &items.balance,
                &items.fund,
                &PredefinedMenuItem::separator(),
                &items.spending,
                &MenuItem::with_id("pay:spending", "Pay for inference…", true, None),
                &PredefinedMenuItem::separator(),
                &items.earning,
                &items.price,
            ])
            .map_err(|e| e.to_string())?;
        let menu = items.menu.clone();
        self.items = Some(items);
        self.shown = Some(l);
        Ok(menu)
    }

    fn render(&mut self) {
        let (Some(items), l) = (&self.items, labels(&self.view)) else {
            return;
        };
        if self.shown.as_ref() == Some(&l) {
            return;
        }
        items.menu.set_text(&l.title);
        items.balance.set_text(&l.balance);
        items.fund.set_text(l.fund);
        items.spending.set_text(&l.spending);
        items.earning.set_text(&l.earning);
        items.price.set_enabled(l.can_price);
        self.shown = Some(l);
    }
}

fn run(client: Client, job: Job) -> Reply {
    match job {
        Job::Refresh { wallet } => Reply::Refreshed {
            balance: wallet.then(|| client.execute(&Command::Balance)),
            policy: client.execute(&Command::Policy { value: None }),
            pricing: client.execute(&Command::Pricing),
        },
        Job::Run(command) => Reply::Done(client.execute(&command)),
        Job::Invoice(amount_msat) => Reply::Invoice(client.execute(&Command::Fund { amount_msat })),
        Job::Receipt(hash) => Reply::Receipt(
            client
                .execute::<Vec<Transaction>>(&Command::Transactions { limit: 200 })
                .map(|txs| payments::receipt(&txs, &hash)),
        ),
    }
}

impl Payments {
    /// `target` is (console port, runtime pid) only while our own runtime is
    /// ready; with no target nothing is sent and last answers stay visible.
    pub fn tick(&mut self, target: Option<(u16, u32)>, models: &[String]) {
        if self.view.models != models {
            self.view.models = models.to_vec();
        }
        while let Ok(reply) = self.replies.try_recv() {
            self.busy = false;
            self.handle(reply, target);
        }
        if let Some(t) = target {
            if !self.busy && Instant::now() >= self.next_refresh {
                self.send(
                    t,
                    Job::Refresh {
                        wallet: self.view.wallet,
                    },
                );
                self.next_refresh = Instant::now() + REFRESH;
            }
        }
        self.render();
    }

    fn send(&mut self, (port, pid): (u16, u32), job: Job) {
        self.busy = self.jobs.send((port, pid, job)).is_ok();
    }

    fn refresh_soon(&mut self) {
        self.next_refresh = Instant::now();
    }

    fn handle(&mut self, reply: Reply, target: Option<(u16, u32)>) {
        match reply {
            Reply::Refreshed {
                balance,
                policy,
                pricing,
            } => {
                if let Some(b) = balance {
                    self.view.balance = Some(b.map(|b| b.spendable_msat).map_err(|e| describe(&e)));
                }
                self.view.policy = Some(policy.map_err(|e| describe(&e)));
                self.view.pricing = Some(pricing.map_err(|e| describe(&e)));
            }
            Reply::Done(Ok(_)) => self.refresh_soon(),
            Reply::Done(Err(e)) => {
                self.refresh_soon();
                ui::notice(
                    "Mesh did not save that",
                    &format!(
                        "{}. Nothing was changed; the menu shows what Mesh reports.",
                        describe(&e)
                    ),
                );
            }
            Reply::Invoice(Ok(inv)) => {
                let amount = inv
                    .amount_msat
                    .map(format_sats)
                    .unwrap_or_else(|| "any amount (payer chooses)".into());
                let minutes = inv.expires_at_ms.saturating_sub(now_ms()) / 60_000;
                let detail = format!("Amount: {amount}\nExpires in about {minutes} min.\n\nScan with a Lightning wallet, or copy the invoice. Your balance updates once the payment arrives.");
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
                self.refresh_soon();
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

    /// Returns true when `id` belonged to Payments.
    pub fn click(&mut self, id: &str, target: Option<(u16, u32)>) -> bool {
        if !id.starts_with("pay:") {
            return false;
        }
        let Some(t) = target else {
            ui::notice(
                "Mesh is still starting",
                "Payments become available once Mesh is running.",
            );
            return true;
        };
        if self.busy {
            ui::notice(
                "One moment",
                "Mesh is still answering the last payments request.",
            );
            return true;
        }
        match id {
            "pay:fund" if !self.view.wallet => {
                if ui::confirm(
                    "Enable wallet?",
                    "Mesh sets up a Bitcoin Lightning wallet in your Mesh profile. It only holds what you add. Nothing is spent unless you turn on Pay for inference.",
                    "Enable wallet",
                ) {
                    self.view.wallet = true;
                    self.view.balance = None;
                    self.send(t, Job::Refresh { wallet: true });
                }
            }
            "pay:fund" => {
                let Some(text) = ui::fund_amount() else {
                    return true;
                };
                let amount = if text.trim().is_empty() {
                    None
                } else {
                    match sats_to_msat(&text) {
                        Ok(msat) => Some(msat),
                        Err(e) => return invalid(&e),
                    }
                };
                self.send(t, Job::Invoice(amount));
            }
            "pay:spending" => self.edit_spending(t),
            "pay:price" => {
                if let Some(model) = self.view.models.first().cloned() {
                    self.edit_price(t, model);
                }
            }
            _ => {}
        }
        true
    }

    fn edit_spending(&mut self, t: (u16, u32)) {
        let (enabled, budget, usage) = match &self.view.policy {
            Some(Ok(p)) => (
                p.mode == Mode::Automatic,
                p.daily_budget_msat
                    .map(|b| (b / 1000).to_string())
                    .unwrap_or_default(),
                format!(
                    "Today: {} spent · {} held for running requests · {} left.",
                    format_sats(p.spent_today_msat),
                    format_sats(p.reserved_msat),
                    format_sats(p.remaining_daily_budget_msat)
                ),
            ),
            _ => (
                false,
                String::new(),
                "Current usage not available yet.".into(),
            ),
        };
        let Some((on, text)) = ui::spending(enabled, &budget, &usage) else {
            return;
        };
        let policy = if on {
            match sats_to_msat(&text) {
                Ok(msat) => Policy {
                    mode: Mode::Automatic,
                    daily_budget_msat: Some(msat),
                },
                Err(e) => {
                    invalid(&e);
                    return;
                }
            }
        } else {
            // Keep the allowance so turning it back on is one click.
            Policy {
                mode: Mode::FreeOnly,
                daily_budget_msat: sats_to_msat(&text).ok(),
            }
        };
        self.send(
            t,
            Job::Run(Command::Policy {
                value: Some(policy),
            }),
        );
    }

    fn edit_price(&mut self, t: (u16, u32), model: String) {
        let current = self
            .view
            .pricing
            .as_ref()
            .and_then(|p| p.as_ref().ok())
            .and_then(|p| p.get(&model))
            .map(|p| {
                [
                    p.input_msat_per_million,
                    p.output_msat_per_million,
                    p.minimum_invoice_msat,
                ]
                .map(|m| (m / 1000).to_string())
            })
            .unwrap_or_else(|| [String::new(), String::new(), "10".into()]);
        let Some(fields) = ui::price(&model, current) else {
            return;
        };
        if fields[..2].iter().all(|f| f.trim().is_empty()) {
            // No input/output price = serve for free: remove the price, keep serving.
            self.send(t, Job::Run(Command::SetPricing { model, value: None }));
            return;
        }
        let parsed: Result<Vec<u64>, _> = fields.iter().map(|f| sats_to_msat(f)).collect();
        match parsed {
            Ok(v) => self.send(
                t,
                Job::Run(Command::SetPricing {
                    model,
                    value: Some(Pricing {
                        input_msat_per_million: v[0],
                        output_msat_per_million: v[1],
                        minimum_invoice_msat: v[2],
                    }),
                }),
            ),
            Err(e) => {
                invalid(&e);
            }
        }
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

    fn status(mode: Mode, remaining: u64) -> PolicyStatus {
        PolicyStatus {
            mode,
            daily_budget_msat: Some(10_000),
            spent_today_msat: 2_000,
            reserved_msat: 3_000,
            remaining_daily_budget_msat: remaining,
        }
    }

    fn view() -> View {
        View {
            wallet: true,
            ..View::default()
        }
    }

    #[test]
    fn no_wallet_keeps_the_same_items_and_offers_enable() {
        let l = labels(&View::default());
        assert_eq!(l.title, "Payments");
        assert_eq!(l.balance, "Wallet not enabled");
        assert_eq!(l.fund, "Enable wallet…");
        assert!(!l.can_price);
    }

    #[test]
    fn balance_is_in_the_title_and_errors_are_not_zero() {
        let mut v = view();
        assert_eq!(labels(&v).balance, "Balance: checking…");
        v.balance = Some(Ok(1_234_000));
        let l = labels(&v);
        assert_eq!(l.title, "Payments · 1,234 sats");
        assert_eq!(l.balance, "Balance: 1,234 sats");
        assert_eq!(l.fund, "Add funds…");
        v.balance = Some(Err("Mesh not reachable".into()));
        let l = labels(&v);
        assert_eq!(l.title, "Payments");
        assert!(l.balance.contains("unavailable"));
    }

    #[test]
    fn spending_label_reflects_mesh_policy() {
        let mut v = view();
        v.policy = Some(Ok(status(Mode::FreeOnly, 0)));
        assert_eq!(labels(&v).spending, "Spending: free only");
        v.policy = Some(Ok(status(Mode::Automatic, 5_000)));
        assert_eq!(labels(&v).spending, "Spending: on · 5 sats left today");
    }

    #[test]
    fn earning_describes_the_served_model_price() {
        let mut v = view();
        assert_eq!(labels(&v).earning, "Earning: no model being served");
        v.models = vec!["m".into()];
        assert!(labels(&v).can_price);
        v.pricing = Some(Ok(BTreeMap::new()));
        assert_eq!(labels(&v).earning, "Earning: free");
        v.pricing = Some(Ok(BTreeMap::from([(
            "m".to_string(),
            Pricing {
                input_msat_per_million: 10_000,
                output_msat_per_million: 20_000,
                minimum_invoice_msat: 1_000,
            },
        )])));
        assert_eq!(
            labels(&v).earning,
            "Earning: 10 sats in · 20 sats out per M tokens"
        );
    }
}
