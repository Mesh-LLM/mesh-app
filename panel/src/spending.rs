//! Inline spending consent: editing a limit never enables payments by itself.
use super::*;
use gpui_kit::component::input::{Input, InputState};

impl Panel {
    pub(super) fn save_spending(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.preview || self.state.busy || self.state.spending_busy {
            self.error = Some("Settings were not saved: Mesh is busy or this is a preview.".into());
            cx.notify();
            return;
        }
        let limit = self
            .daily_limit
            .as_ref()
            .map(|v| v.read(cx).value().to_string())
            .unwrap_or_default();
        if enabled {
            if let Err(e) = mesh_tray::payments::sats_to_msat(&limit) {
                self.error = Some(format!("Enter a positive daily limit in whole sats: {e:?}"));
                cx.notify();
                return;
            }
        }
        self.error = panel_ipc::send_action(
            &self.root,
            &panel_ipc::Action::SetSpending {
                enabled,
                daily_limit_sats: limit,
            },
        )
        .err();
        cx.notify();
    }

    pub(super) fn spending_card(
        &mut self,
        palette: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let st = self.state.clone();
        if self.daily_limit.is_none() {
            let value = st
                .spending
                .as_ref()
                .and_then(|p| p.daily_limit_msat)
                .map(|n| (n / 1000).to_string())
                .unwrap_or_default();
            self.daily_limit = Some(cx.new(|cx| InputState::new(window, cx).default_value(value)));
        }
        let input = self.daily_limit.as_ref().expect("created above");
        let enabled = st.spending.as_ref().is_some_and(|p| p.enabled);
        let disabled = st.busy || st.spending_busy || st.spending.is_none();
        card(palette, "Pay for models")
            .child(
                switch("pay-enabled", "Allow paid models", enabled)
                    .disabled(disabled)
                    .on_click(cx.listener(move |p, _, _, cx| p.save_spending(!enabled, cx))),
            )
            .child(muted(
                palette,
                if enabled {
                    "Paid models are allowed within your daily allowance."
                } else {
                    "Off uses free models only. Adding funds never turns this on."
                },
            ))
            .child(div().child("Daily limit (sats · resets at midnight UTC)"))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Input::new(input)
                            .id("daily-limit")
                            .w(px(180.))
                            .disabled(st.busy || st.spending_busy),
                    )
                    .child(
                        chip("save-limit", "Save limit", false)
                            .disabled(disabled)
                            .on_click(cx.listener(move |p, _, _, cx| p.save_spending(enabled, cx))),
                    ),
            )
            .when_some(st.spending.as_ref(), |d, p| {
                d.child(muted(
                    palette,
                    format!(
                        "Today: {} spent · {} remaining",
                        mesh_tray::payments::format_sats(p.spent_msat),
                        mesh_tray::payments::format_sats(p.remaining_msat)
                    ),
                ))
            })
            .child(muted(
                palette,
                "The limit stops new paid requests. Requests already running may finish above it.",
            ))
            .when(st.spending_busy, |d| {
                d.child(muted(palette, "Checking payment settings…"))
            })
            .when_some(st.spending_error.clone(), |d, e| d.child(div().child(e)))
            .child(div().child(match (st.wallet, st.balance_sats) {
                (true, Some(b)) => format!("Balance: {} sats{}", sats(b), usd(b, &st)),
                (true, None) => "Balance unavailable".into(),
                (false, _) => "No wallet yet. Add funds to set one up.".into(),
            }))
            .child(
                chip("fund", "Add funds (QR)…", false)
                    .disabled(st.busy)
                    .on_click(cx.listener(|p, _, _, cx| p.act("pay:fund", cx))),
            )
    }
}
