//! Follow-up actions when something needs to be done after a check has been performed.

use crate::prelude::*;

pub(crate) mod pushover;

#[async_trait]
/// An action that'll run after a check has been performed
pub trait Action {
    /// Run the response action
    async fn execute(&self, check_result: &CheckResult) -> Result<(), Error>;

    /// What states the action would be run
    fn run_states(&self) -> Vec<ServiceStatus>;
}
