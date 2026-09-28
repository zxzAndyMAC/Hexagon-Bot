//! D08: explicit scripted budget fixtures are separate from the paid authority.
use super::*;
use crate::evaluation as eval;
impl Workbench {
    /// Owner-supplied supplier evidence; never exposed as an agent tool.
    pub fn reconcile_evaluation_billing(
        &self,
        evidence: &eval::SupplierUsageReconciliation,
    ) -> Result<eval::BudgetSummary, ApiError> {
        Ok(eval::budget::reconciliation::reconcile(
            &self.repo_root,
            evidence,
        )?)
    }
    pub fn evaluation_billing_requests(&self) -> Result<Vec<eval::BillingRequest>, ApiError> {
        Ok(eval::budget::reconciliation::requests(&self.repo_root)?)
    }

    pub fn enable_evaluation_budget_debug(
        &self,
        plan: &str,
        price: eval::DebugPrice,
    ) -> Result<(), ApiError> {
        Ok(eval::budget::enable_debug(&self.db, plan, &price)?)
    }
    pub fn evaluation_budget_debug(&self) -> Result<eval::BudgetSummary, ApiError> {
        Ok(eval::budget::summary(&self.db, "scripted_debug")?)
    }
    pub fn evaluation_budget(&self) -> Result<eval::BudgetSummary, ApiError> {
        Ok(eval::budget::paid_summary()?)
    }
}

#[cfg(test)]
impl Workbench {
    pub(super) fn reserve_evaluation_paid_fixture(
        &self,
        authority: &Path,
        plan: &str,
        price: &eval::DebugPrice,
    ) -> Result<eval::BudgetSummary, ApiError> {
        Ok(eval::budget::reserve_paid_fixture(
            &self.db,
            &self.repo_root,
            authority,
            plan,
            price,
        )?)
    }
}

#[cfg(test)]
impl Workbench {
    pub(super) fn redeliver_evaluation_receipt_fixture(
        &self,
        run_key: &str,
        usage: &crate::provider::Usage,
    ) -> Result<(), ApiError> {
        Ok(eval::budget::redeliver_fixture(
            &self.repo_root,
            run_key,
            usage,
        )?)
    }
}
