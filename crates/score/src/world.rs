//! One world as the scorer sees it: its dataset, checked inputs, labels and
//! coverage.

use a2a_bench_format::check::WorldInputs;
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::DatasetId;
use a2a_bench_format::labels::Label;

/// A world to judge predictions against. `inputs` are checked
/// (`WorldInputs::new`) and `labels` checked against them
/// (`check_labels`) before scoring.
#[derive(Debug, Clone, Copy)]
pub struct World<'w> {
    pub dataset: &'w DatasetId,
    pub inputs: &'w WorldInputs,
    pub labels: &'w [Label],
    pub coverage: Coverage,
}
