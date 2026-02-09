use fchat::ChatTurnResult;
use fcommon::{BoxFuture, SessionId};
use fmemory::{FeatureRecord, InitPlan};

use crate::HarnessError;

pub trait HealthChecker: Send + Sync {
    fn run<'a>(
        &'a self,
        session_id: &'a SessionId,
        init_plan: &'a InitPlan,
    ) -> BoxFuture<'a, Result<(), HarnessError>>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopHealthChecker;

impl HealthChecker for NoopHealthChecker {
    fn run<'a>(
        &'a self,
        _session_id: &'a SessionId,
        _init_plan: &'a InitPlan,
    ) -> BoxFuture<'a, Result<(), HarnessError>> {
        Box::pin(async { Ok(()) })
    }
}

pub trait OutcomeValidator: Send + Sync {
    fn validate<'a>(
        &'a self,
        feature: &'a FeatureRecord,
        result: &'a ChatTurnResult,
    ) -> BoxFuture<'a, Result<bool, HarnessError>>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct AcceptAllValidator;

impl OutcomeValidator for AcceptAllValidator {
    fn validate<'a>(
        &'a self,
        _feature: &'a FeatureRecord,
        _result: &'a ChatTurnResult,
    ) -> BoxFuture<'a, Result<bool, HarnessError>> {
        Box::pin(async { Ok(true) })
    }
}

pub trait FeatureSelector: Send + Sync {
    fn select(&self, feature_list: &[FeatureRecord]) -> Option<FeatureRecord>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct FirstPendingFeatureSelector;

impl FeatureSelector for FirstPendingFeatureSelector {
    fn select(&self, feature_list: &[FeatureRecord]) -> Option<FeatureRecord> {
        feature_list.iter().find(|feature| !feature.passes).cloned()
    }
}
