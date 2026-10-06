//! Helpers over bench worlds for the ported tests.

use a2a_bench_corpus::world::World;
use a2a_bench_dataset_ai_village::shell::locator::Loc;
use a2a_bench_format::exchange::Exchange;
use a2a_bench_format::ids::ExchangeId;
use a2a_bench_format::labels::{Label, TransmissionFields};
use a2a_bench_format::location::Location;
use a2a_bench_format::message::Message;
use a2a_bench_format::resource::Resource;

/// Every transmission label of a world (content labels), in order.
pub fn transmissions(world: &World) -> Vec<&TransmissionFields> {
    world
        .labels()
        .iter()
        .filter_map(|label| match label {
            Label::Transmission(t) => Some(t.fields()),
            _ => None,
        })
        .collect()
}

/// Every repository label, content or access-only.
pub fn channel_labels(world: &World) -> Vec<(&TransmissionFields, bool)> {
    world
        .labels()
        .iter()
        .filter_map(|label| match label {
            Label::Transmission(t) => Some((t.fields(), false)),
            Label::AccessOnly(t) => Some((t.fields(), true)),
            _ => None,
        })
        .collect()
}

pub fn exchange(world: &World, id: ExchangeId) -> &Exchange {
    world
        .exchange(id)
        .unwrap_or_else(|| panic!("no exchange {id:?}"))
}

/// The agent name that made an exchange.
pub fn agent(world: &World, id: ExchangeId) -> String {
    world
        .agent_of(id)
        .map(|key| key.as_str().to_owned())
        .unwrap_or_default()
}

/// A world's exchanges of the agent named `name`, in time order.
pub fn exchanges_of<'a>(world: &'a World, name: &str) -> Vec<&'a Exchange> {
    world
        .exchanges()
        .iter()
        .filter(|e| world.agent_of(e.id).is_some_and(|key| key.as_str() == name))
        .collect()
}

/// The request's messages.
pub fn request<'a>(world: &'a World, exchange: &Exchange) -> Vec<&'a Message> {
    exchange
        .request
        .messages
        .iter()
        .map(|id| {
            world
                .message(*id)
                .unwrap_or_else(|| panic!("message {id:?}"))
        })
        .collect()
}

/// The text a location cuts.
pub fn text_at(world: &World, at: &Location) -> String {
    let message = world
        .message(at.message)
        .unwrap_or_else(|| panic!("message"));
    let text = message.part_text(at.part).unwrap_or_else(|e| panic!("{e}"));
    text[at.range.start() as usize..at.range.end() as usize].to_owned()
}

/// A resource in the locator notation (`repo://…`, `file://…`, the URL).
pub fn key(resource: &Resource) -> String {
    Loc::of_site(resource)
        .map(|loc| loc.to_string())
        .unwrap_or_else(|| panic!("{resource:?}"))
}
