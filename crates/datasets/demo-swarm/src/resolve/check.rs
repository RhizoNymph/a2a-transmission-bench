//! Each label checked against the world before it is kept, with the same
//! checks a reader applies ([`check_labels`]), so one label the format
//! refuses is a diagnostic, not a world that fails to build.

use a2a_bench_format::check::{LabelError, WorldInputs, check_labels};
use a2a_bench_format::labels::Label;

/// The world's inputs and its `exchange_agent` rows.
pub(crate) struct Checker {
    inputs: WorldInputs,
    rows: Vec<Label>,
}

impl Checker {
    /// `agent_rows` are the world's `exchange_agent` rows.
    pub(crate) fn new(inputs: WorldInputs, agent_rows: Vec<Label>) -> Self {
        Self {
            inputs,
            rows: agent_rows,
        }
    }

    /// Checks `label` in this world, beside the agent rows only.
    pub(crate) fn check(&mut self, label: &Label) -> Result<(), LabelError> {
        let base = self.rows.len();
        self.rows.push(label.clone());
        let checked = check_labels(&self.inputs, &self.rows);
        self.rows.truncate(base);
        checked
    }

    /// The agent rows.
    pub(crate) fn into_rows(self) -> Vec<Label> {
        self.rows
    }
}
