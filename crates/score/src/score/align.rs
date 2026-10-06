//! The alignment rule: when a prediction is the transmission a label
//! expects. Every count derives from it.

use a2a_bench_format::labels::{Exemption, ExpectedTransmission, NegativeControl, Route};
use a2a_bench_format::predictions::PredictedRoute;

use crate::canon::Canonicalize;
use crate::predict::Prediction;

/// Whether `prediction` reports the transmission `expected` labels.
///
/// All of:
///
/// 1. the same sender (`from`) and reader (`to`) agent;
/// 2. the same reader exchange: the content is found where it first arrived,
///    not in a later exchange that merely still carries it;
/// 3. overlapping content locations: the matched reader text shares at least
///    one byte with the labelled text (same message, same part);
/// 4. for a label routed through a channel, a predicted channel one of whose
///    resources is the label's resource, compared after `canon`. Other route kinds do not have to
///    agree: a detector that finds the content but routes it differently is
///    still credited, and the per-route breakdown shows the disagreement.
///
/// Match class and carrier never decide alignment; they only choose the row
/// a prediction and a label are counted in.
pub fn aligns(
    prediction: &Prediction,
    expected: &ExpectedTransmission,
    canon: &dyn Canonicalize,
) -> bool {
    let label = expected.fields();
    prediction.from == label.from
        && prediction.to == label.to
        && prediction.reader_exchange == label.reader_exchange
        && prediction.read_at.overlaps(&label.content.at)
        && same_channel(&label.route, &prediction.route, canon)
}

fn same_channel(expected: &Route, predicted: &PredictedRoute, canon: &dyn Canonicalize) -> bool {
    match (expected, predicted) {
        (Route::Channel { resource: label }, PredictedRoute::Channel { resources }) => {
            let label = canon.canonical(label);
            resources
                .iter()
                .any(|resource| canon.canonical(resource) == label)
        }
        (Route::Channel { .. }, _) => false,
        (Route::Delegation { .. } | Route::Direct | Route::Unobserved, _) => true,
    }
}

/// Whether a prediction that aligns with no label falls under `control`:
/// same sender and reader, the control's reader exchange (when it names
/// one), a read location overlapping the control's (when it names one), and
/// a matched origin overlapping the control's origin (when it names one; a
/// prediction whose origin is unknown never falls under such a control).
pub fn violates(prediction: &Prediction, control: &NegativeControl) -> bool {
    let label = control.fields();
    prediction.from == label.from
        && prediction.to == label.to
        && label
            .reader_exchange
            .is_none_or(|exchange| exchange == prediction.reader_exchange)
        && label
            .at
            .as_ref()
            .is_none_or(|at| at.overlaps(&prediction.read_at))
        && label.origin.as_ref().is_none_or(|origin| {
            prediction
                .origin_at
                .as_ref()
                .is_some_and(|span| span.overlaps(origin))
        })
}

/// How specific a control is: one naming a read location is checked first,
/// then one naming an origin, then one naming only an exchange, so a
/// prediction is charged to the most specific control it falls under.
pub fn specificity(control: &NegativeControl) -> u8 {
    let label = control.fields();
    match (
        label.at.is_some(),
        label.origin.is_some(),
        label.reader_exchange.is_some(),
    ) {
        (true, _, _) => 0,
        (false, true, _) => 1,
        (false, false, true) => 2,
        (false, false, false) => 3,
    }
}

/// Whether a prediction that aligns with no label falls under `exemption`:
/// the same reader and reader exchange, and an overlapping read location.
/// The sender is not compared: the exemption is for content whose sender
/// is unknown.
pub fn exempts(prediction: &Prediction, exemption: &Exemption) -> bool {
    let label = exemption.fields();
    prediction.to == label.to
        && prediction.reader_exchange == label.reader_exchange
        && prediction.read_at.overlaps(&label.at)
}
