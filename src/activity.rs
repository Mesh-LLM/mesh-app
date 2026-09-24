//! What the menu-bar jellyfish shows: quiet when idle, lit while this node is
//! handling requests, gold once it has been paid.
//!
//! Idle keeps the template icon so it matches every other menu-bar item. The
//! lit states turn template mode off, because macOS discards the colour of a
//! template image. Both colours read on light and dark menu bars.
//!
//! "Paid" is inferred from the wallet's spendable balance going up; Mesh has no
//! per-request paid flag. A funding invoice the user pays also raises it, which
//! lights gold once — acceptable, since that is money arriving too.
use std::time::{Duration, Instant};

/// Keep "in use" this long after the last busy poll, so short requests do not
/// blink the icon on and off between 3s polls.
const BUSY_HOLD: Duration = Duration::from_secs(6);
/// Keep "earning" this long after the last balance increase. The balance is
/// read every 10s, so a payment is always visible for several reads.
const PAID_HOLD: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    InUse,
    Earning,
}

const TEAL: [u8; 3] = [0x1f, 0xb8, 0xc4];
const GOLD: [u8; 3] = [0xf2, 0xa9, 0x1c];

#[derive(Default)]
pub struct Activity {
    last_busy: Option<Instant>,
    last_paid: Option<Instant>,
    balance: Option<u64>,
}

impl Activity {
    /// Record one status poll. `inflight` is `/api/status` `inflight_requests`.
    pub fn busy(&mut self, inflight: u64, now: Instant) {
        if inflight > 0 {
            self.last_busy = Some(now);
        }
    }

    /// Record the latest good balance. The first reading only sets a baseline:
    /// money already in the wallet at startup is not a payment.
    pub fn balance(&mut self, balance: Option<u64>, now: Instant) {
        let Some(balance) = balance else { return };
        if self.balance.is_some_and(|before| balance > before) {
            self.last_paid = Some(now);
        }
        self.balance = Some(balance);
    }

    /// The engine stopped: nothing it reported is current any more.
    pub fn reset(&mut self) {
        self.last_busy = None;
        self.last_paid = None;
    }

    pub fn state(&self, now: Instant) -> State {
        let within = |at: Option<Instant>, hold| at.is_some_and(|at| now.duration_since(at) < hold);
        if within(self.last_paid, PAID_HOLD) {
            State::Earning
        } else if within(self.last_busy, BUSY_HOLD) {
            State::InUse
        } else {
            State::Idle
        }
    }
}

/// `(rgba, is_template)` for a state, from the jellyfish's alpha silhouette.
pub fn artwork(mask: &[u8], state: State) -> (Vec<u8>, bool) {
    let colour = match state {
        State::Idle => return (mask.to_vec(), true),
        State::InUse => TEAL,
        State::Earning => GOLD,
    };
    let rgba = mask
        .chunks_exact(4)
        .flat_map(|px| [colour[0], colour[1], colour[2], px[3]])
        .collect();
    (rgba, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_until_something_happens() {
        let now = Instant::now();
        let mut a = Activity::default();
        a.busy(0, now);
        a.balance(Some(5_000), now);
        assert_eq!(a.state(now), State::Idle);
    }

    #[test]
    fn in_use_holds_across_a_poll_gap_then_drops() {
        let t = Instant::now();
        let mut a = Activity::default();
        a.busy(1, t);
        a.busy(0, t + Duration::from_secs(3));
        assert_eq!(a.state(t + Duration::from_secs(3)), State::InUse);
        assert_eq!(a.state(t + BUSY_HOLD), State::Idle);
    }

    #[test]
    fn a_balance_increase_is_earning_and_outranks_in_use() {
        let t = Instant::now();
        let mut a = Activity::default();
        a.balance(Some(1_000), t);
        a.busy(2, t);
        a.balance(Some(22_000), t + Duration::from_secs(10));
        assert_eq!(a.state(t + Duration::from_secs(10)), State::Earning);
        assert_eq!(
            a.state(t + Duration::from_secs(10) + PAID_HOLD),
            State::Idle
        );
    }

    #[test]
    fn spending_or_a_failed_read_is_not_earning() {
        let t = Instant::now();
        let mut a = Activity::default();
        a.balance(Some(9_000), t);
        a.balance(None, t);
        a.balance(Some(4_000), t);
        assert_eq!(a.state(t), State::Idle);
        // A failed read does not reset the baseline, so recovering is not a payment.
        a.balance(None, t);
        a.balance(Some(4_000), t);
        assert_eq!(a.state(t), State::Idle);
    }

    #[test]
    fn reset_clears_lit_states() {
        let t = Instant::now();
        let mut a = Activity::default();
        a.balance(Some(1), t);
        a.balance(Some(2), t);
        a.reset();
        assert_eq!(a.state(t), State::Idle);
    }

    #[test]
    fn colour_keeps_the_silhouette() {
        let mask = [0, 0, 0, 0, 0, 0, 0, 255];
        let (idle, template) = artwork(&mask, State::Idle);
        assert!(template);
        assert_eq!(idle, mask);
        let (gold, template) = artwork(&mask, State::Earning);
        assert!(!template);
        assert_eq!(gold, [0xf2, 0xa9, 0x1c, 0, 0xf2, 0xa9, 0x1c, 255]);
    }
}
