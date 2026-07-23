use serde::{Deserialize, Serialize};

const MAX_ARCHIVED_TRANSACTION_HISTORY: usize = 32;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RechargeStage {
    Prepared,
    TransferSubmitted,
    OrderCreated,
    HandedOff,
    PaymentConfirmed,
    Completed,
    Unknown,
    Cancelled,
}

impl RechargeStage {
    pub(crate) fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::TransferSubmitted
                | Self::OrderCreated
                | Self::HandedOff
                | Self::PaymentConfirmed
                | Self::Unknown
        )
    }

    fn can_transition_to(&self, next: &Self) -> bool {
        use RechargeStage::*;
        matches!(
            (self, next),
            (
                Prepared,
                TransferSubmitted | OrderCreated | HandedOff | Cancelled | Unknown
            ) | (TransferSubmitted, Completed | Unknown)
                | (
                    OrderCreated,
                    HandedOff | PaymentConfirmed | Completed | Unknown | Cancelled
                )
                | (
                    HandedOff,
                    PaymentConfirmed | Completed | Unknown | Cancelled
                )
                | (PaymentConfirmed, TransferSubmitted | Completed | Unknown)
                | (
                    Unknown,
                    PaymentConfirmed | TransferSubmitted | Completed | Cancelled
                )
        ) || self == next
    }
}

pub(crate) fn stage_after_payment_context_closed(
    stage: RechargeStage,
) -> Option<(RechargeStage, &'static str)> {
    match stage {
        RechargeStage::Prepared => Some((RechargeStage::Cancelled, "微信订单创建前已取消")),
        RechargeStage::OrderCreated | RechargeStage::HandedOff => Some((
            RechargeStage::Unknown,
            "微信支付入口已关闭，订单最终结果仍需核对",
        )),
        RechargeStage::PaymentConfirmed => Some((
            RechargeStage::PaymentConfirmed,
            "微信付款已确认，仍等待转入目标网费账户",
        )),
        RechargeStage::TransferSubmitted | RechargeStage::Unknown => {
            Some((RechargeStage::Unknown, "充值流程结果仍需核对"))
        }
        RechargeStage::Completed | RechargeStage::Cancelled => None,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RechargeTransaction {
    pub id: String,
    pub method: String,
    pub payer_account: String,
    pub target_account: String,
    pub amount: String,
    pub stage: RechargeStage,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub card_balance_before: String,
    #[serde(default)]
    pub payment_url: String,
    #[serde(default)]
    pub payment_id: String,
    #[serde(default)]
    pub partner_jour_no: String,
    #[serde(default)]
    pub openid: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RechargeRecoveryView {
    pub id: String,
    pub method: String,
    pub payer_account: String,
    pub target_account: String,
    pub amount: String,
    pub stage: RechargeStage,
    pub card_balance_before: String,
    pub payment_url: String,
    pub payment_id: String,
    pub note: String,
}

impl From<&RechargeTransaction> for RechargeRecoveryView {
    fn from(transaction: &RechargeTransaction) -> Self {
        Self {
            id: transaction.id.clone(),
            method: transaction.method.clone(),
            payer_account: transaction.payer_account.clone(),
            target_account: transaction.target_account.clone(),
            amount: transaction.amount.clone(),
            stage: transaction.stage.clone(),
            card_balance_before: transaction.card_balance_before.clone(),
            payment_url: transaction.payment_url.clone(),
            payment_id: transaction.payment_id.clone(),
            note: transaction.note.clone(),
        }
    }
}

impl RechargeTransaction {
    pub(crate) fn prepared(
        id: String,
        method: &str,
        payer_account: String,
        target_account: String,
        amount: String,
        card_balance_before: String,
        now: i64,
    ) -> Self {
        Self {
            id,
            method: method.to_string(),
            payer_account,
            target_account,
            amount,
            stage: RechargeStage::Prepared,
            created_at: now,
            updated_at: now,
            card_balance_before,
            payment_url: String::new(),
            payment_id: String::new(),
            partner_jour_no: String::new(),
            openid: String::new(),
            note: String::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub(crate) struct RechargeJournal(pub Vec<RechargeTransaction>);

impl RechargeJournal {
    pub(crate) fn upsert(&mut self, transaction: RechargeTransaction) {
        if let Some(current) = self.0.iter_mut().find(|item| item.id == transaction.id) {
            *current = transaction;
        } else {
            self.0.push(transaction);
        }
        self.trim();
    }

    pub(crate) fn transition(
        &mut self,
        id: &str,
        stage: RechargeStage,
        now: i64,
        note: impl Into<String>,
    ) -> Result<(), String> {
        let transaction = self
            .0
            .iter_mut()
            .find(|item| item.id == id || (!item.payment_id.is_empty() && item.payment_id == id))
            .ok_or_else(|| "找不到对应的充值恢复记录".to_string())?;
        if !transaction.stage.can_transition_to(&stage) {
            return Err(format!(
                "不允许的充值状态迁移：{:?} -> {:?}",
                transaction.stage, stage
            ));
        }
        transaction.stage = stage;
        transaction.updated_at = now;
        transaction.note = note.into();
        self.trim();
        Ok(())
    }

    pub(crate) fn recovery_views(&self) -> Vec<RechargeRecoveryView> {
        let mut items = self
            .0
            .iter()
            .filter(|item| item.stage.is_recoverable())
            .map(|item| (item.updated_at, RechargeRecoveryView::from(item)))
            .collect::<Vec<_>>();
        items.sort_by_key(|(updated_at, _)| std::cmp::Reverse(*updated_at));
        items.into_iter().map(|(_, view)| view).collect()
    }

    pub(crate) fn find(&self, id: &str) -> Option<&RechargeTransaction> {
        self.0
            .iter()
            .find(|item| item.id == id || (!item.payment_id.is_empty() && item.payment_id == id))
    }

    fn trim(&mut self) {
        self.0
            .sort_by_key(|item| std::cmp::Reverse(item.updated_at));
        let mut archived_kept = 0;
        self.0.retain(|item| {
            if matches!(
                item.stage,
                RechargeStage::Completed | RechargeStage::Cancelled
            ) {
                archived_kept += 1;
                archived_kept <= MAX_ARCHIVED_TRANSACTION_HISTORY
            } else {
                true
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transaction() -> RechargeTransaction {
        RechargeTransaction::prepared(
            "tx-1".to_string(),
            "wechat",
            "25000001".to_string(),
            "25000002".to_string(),
            "10.00".to_string(),
            "20.00".to_string(),
            1,
        )
    }

    #[test]
    fn payment_state_machine_accepts_recovery_path() {
        let mut journal = RechargeJournal::default();
        journal.upsert(transaction());
        journal
            .transition("tx-1", RechargeStage::OrderCreated, 2, "created")
            .unwrap();
        journal
            .transition("tx-1", RechargeStage::HandedOff, 3, "external app")
            .unwrap();
        journal
            .transition("tx-1", RechargeStage::PaymentConfirmed, 4, "paid")
            .unwrap();
        journal
            .transition("tx-1", RechargeStage::TransferSubmitted, 5, "transfer")
            .unwrap();
        journal
            .transition("tx-1", RechargeStage::Completed, 6, "done")
            .unwrap();
        assert!(journal.recovery_views().is_empty());
    }

    #[test]
    fn unknown_result_remains_recoverable_and_blocks_new_terminal_transition() {
        let mut journal = RechargeJournal::default();
        journal.upsert(transaction());
        journal
            .transition("tx-1", RechargeStage::TransferSubmitted, 2, "sent")
            .unwrap();
        journal
            .transition("tx-1", RechargeStage::Unknown, 3, "timeout")
            .unwrap();
        assert_eq!(journal.recovery_views().len(), 1);
        assert!(journal
            .transition("tx-1", RechargeStage::OrderCreated, 4, "invalid")
            .is_err());
    }

    #[test]
    fn archived_journal_is_bounded_without_dropping_recoverable_records() {
        let mut journal = RechargeJournal::default();
        for index in 0..40 {
            let mut item = transaction();
            item.id = format!("archived-{index}");
            item.updated_at = index;
            item.stage = RechargeStage::Cancelled;
            journal.upsert(item);
        }
        for index in 0..40 {
            let mut item = transaction();
            item.id = format!("pending-{index}");
            item.updated_at = 100 + index;
            item.stage = RechargeStage::Unknown;
            journal.upsert(item);
        }
        assert_eq!(
            journal
                .0
                .iter()
                .filter(|item| item.stage == RechargeStage::Cancelled)
                .count(),
            MAX_ARCHIVED_TRANSACTION_HISTORY
        );
        assert_eq!(journal.recovery_views().len(), 40);
    }

    #[test]
    fn restored_payment_can_be_found_and_completed_by_provider_id() {
        let mut item = transaction();
        item.payment_id = "wx-order-1".to_string();
        item.stage = RechargeStage::HandedOff;
        let encoded = serde_json::to_string(&RechargeJournal(vec![item])).unwrap();
        let mut restored: RechargeJournal = serde_json::from_str(&encoded).unwrap();

        assert_eq!(
            restored.find("wx-order-1").map(|item| item.id.as_str()),
            Some("tx-1")
        );
        restored
            .transition(
                "wx-order-1",
                RechargeStage::PaymentConfirmed,
                7,
                "provider confirmed",
            )
            .unwrap();
        restored
            .transition("wx-order-1", RechargeStage::Completed, 8, "transferred")
            .unwrap();
        assert!(restored.recovery_views().is_empty());
    }

    #[test]
    fn recovery_view_does_not_expose_session_secrets() {
        let mut item = transaction();
        item.stage = RechargeStage::HandedOff;
        item.openid = "secret-openid".to_string();
        item.partner_jour_no = "secret-journal".to_string();
        let journal = RechargeJournal(vec![item]);

        let serialized = serde_json::to_string(&journal.recovery_views()).unwrap();
        assert!(!serialized.contains("openid"));
        assert!(!serialized.contains("partnerJourNo"));
        assert!(!serialized.contains("secret-openid"));
        assert!(!serialized.contains("secret-journal"));
    }

    #[test]
    fn closing_payment_context_never_marks_confirmed_payment_completed() {
        assert_eq!(
            stage_after_payment_context_closed(RechargeStage::PaymentConfirmed)
                .map(|(stage, _)| stage),
            Some(RechargeStage::PaymentConfirmed)
        );
        assert_eq!(
            stage_after_payment_context_closed(RechargeStage::HandedOff).map(|(stage, _)| stage),
            Some(RechargeStage::Unknown)
        );
        assert_eq!(
            stage_after_payment_context_closed(RechargeStage::Completed),
            None
        );
    }
}
