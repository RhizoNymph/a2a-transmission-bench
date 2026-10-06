//! Checks across one world's files: a world's inputs (messages and
//! exchanges), its labels against its inputs, and a detector's predictions
//! against its inputs. A row's own checks are in its constructor; these
//! need the world.

mod inputs;
mod labels;
mod location;
mod predictions;

pub use inputs::{InputError, WorldInputs};
pub use labels::{LabelError, check_labels};
pub use location::LocationError;
pub use predictions::{PredictionError, check_predictions};
