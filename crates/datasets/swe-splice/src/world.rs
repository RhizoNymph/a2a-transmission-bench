//! The world of one planned splice.

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::helpers::background::{BackgroundWorld, Trajectory};
use a2a_bench_corpus::helpers::chat::convert;
use a2a_bench_corpus::world::{CorpusError, World};
use a2a_bench_dataset_open_swe::{OpenSweError, agent_name, calls};
use a2a_bench_format::ids::{DatasetId, LabelId, SourceRef, WorldKey};
use a2a_bench_format::labels::{
    CarrierKind, ExpectedContent, ExpectedTransmission, Label, Route, TransmissionFields,
};
use a2a_bench_format::location::{ByteRange, Location};
use a2a_bench_format::resource::Resource;

use crate::DATASET;
use crate::error::SpliceError;
use crate::plan::Plan;
use crate::pool::{Pooled, calls_before, rewrite_workdir};
use crate::read::ReadForm;
use crate::write::{self, FileWrite};

/// The world of `plan` over `pool`, calls `pace` apart: the sender moved
/// into the reader's working directory, the read spliced into the reader,
/// the clocks offset so the read call follows the writing call, and the one
/// label (`splice/<number>`) beside the background controls.
pub fn world(pool: &[Pooled], plan: &Plan, pace: Pace) -> Result<World, SpliceError> {
    let no_writer = || SpliceError::NoWriter(plan.number);
    let no_reader = || SpliceError::NoReader(plan.number);
    let sender = pool.get(plan.sender).ok_or_else(no_writer)?;
    let reader = pool.get(plan.reader).ok_or_else(no_reader)?;
    let form = ReadForm::of(&reader.record.messages);

    // The sender, moved into the reader's working directory.
    let chosen = sender
        .writes()
        .into_iter()
        .nth(plan.write)
        .ok_or_else(no_writer)?;
    let sent = rewrite_workdir(&sender.record.messages, &sender.workdir, &reader.workdir);
    let same_call = |w: &FileWrite| w.message == chosen.message && w.call == chosen.call;
    let nth_in_call = write::writes(&sender.record.messages, &sender.workdir)
        .into_iter()
        .filter(same_call)
        .position(|w| w == chosen)
        .ok_or_else(no_writer)?;
    let written = write::writes(&sent, &reader.workdir)
        .into_iter()
        .filter(same_call)
        .nth(nth_in_call)
        .ok_or_else(no_writer)?;

    // The reader, with the read spliced in.
    let body = plan.variant.render(&written.content);
    let (result, (start, end)) = form.result(&written.path, &plan.call_id, &body);
    let mut read = reader.record.messages.clone();
    if plan.insert_at > read.len() {
        return Err(no_reader());
    }
    read.splice(
        plan.insert_at..plan.insert_at,
        [form.call(&written.path, &plan.call_id), result],
    );

    // Clocks: the read call right after the writing call.
    let write_call = calls_before(&sent, written.message);
    let read_call = calls_before(&read, plan.insert_at);
    let (a0, b0) = if read_call > write_call {
        (read_call - write_call - 1, 0)
    } else {
        (0, write_call + 1 - read_call)
    };
    let a_file = sender.shard.relative.clone();
    let b_file = reader.shard.relative.clone();
    let sender_calls = calls(&sent, &a_file, sender.row, |i| pace.at(a0 + i, 1, 0))?;
    let reader_calls = calls(&read, &b_file, reader.row, |j| pace.at(b0 + j, 0, 0))?;

    // The result message and the numbered lines in it.
    let converted = convert(&read).map_err(|source| OpenSweError::Chat {
        file: b_file.clone(),
        row: reader.row,
        source,
    })?;
    let carrying = converted.get(plan.insert_at + 1).ok_or_else(no_reader)?;
    let out_of_text = || SpliceError::OutOfText {
        number: plan.number,
        start,
        end,
    };
    let text = carrying
        .part_text(0)
        .map_err(|source| SpliceError::Text {
            number: plan.number,
            source,
        })?
        .get(start..end)
        .ok_or_else(out_of_text)?
        .to_owned();
    let range = ByteRange::new(
        u32::try_from(start).map_err(|_| out_of_text())?,
        u32::try_from(end).map_err(|_| out_of_text())?,
    )
    .map_err(|source| SpliceError::Range {
        number: plan.number,
        source,
    })?;

    let dataset = DatasetId::new(DATASET).map_err(CorpusError::from)?;
    let key = WorldKey::new(format!(
        "splice-{:04}-{}-{}",
        plan.number, plan.variant, form
    ))
    .map_err(CorpusError::from)?;
    let mut world = BackgroundWorld::new(dataset, key);
    let (from, sender_ids) = world.add(Trajectory {
        name: format!("sender/{}", agent_name(&sender.shard, sender.row)),
        model: sender.shard.model.clone(),
        group: sender.record.repo.clone(),
        calls: sender_calls,
    })?;
    let (to, reader_ids) = world.add(Trajectory {
        name: format!("reader/{}", agent_name(&reader.shard, reader.row)),
        model: reader.shard.model.clone(),
        group: reader.record.repo.clone(),
        calls: reader_calls,
    })?;
    let reader_exchange = usize::try_from(read_call + 1)
        .ok()
        .and_then(|at| reader_ids.get(at).copied())
        .ok_or_else(no_reader)?;
    let sender_exchange = usize::try_from(write_call)
        .ok()
        .and_then(|at| sender_ids.get(at).copied());
    let (needs, tier) = plan.variant.need(form);
    let label = ExpectedTransmission::new(TransmissionFields {
        id: LabelId::new(format!("splice/{}", plan.number)).map_err(CorpusError::from)?,
        from: from.key().clone(),
        to: to.key().clone(),
        sender_exchange,
        reader_exchange,
        route: Route::Channel {
            resource: Resource::File {
                path: written.path.clone(),
            },
        },
        carrier: CarrierKind::ToolResult,
        content: ExpectedContent {
            text,
            at: Location {
                exchange: reader_exchange,
                message: carrying.id(),
                part: 0,
                range,
            },
        },
        needs,
        tier,
        source: SourceRef::new(
            b_file,
            format!(
                "/rows/{}/messages/{}/splice/{}/from/{}/rows/{}/messages/{}",
                reader.row,
                plan.insert_at + 1,
                plan.number,
                a_file,
                sender.row,
                written.message
            ),
        ),
    })
    .map_err(CorpusError::from)?;
    world.label(Label::Transmission(label))?;
    Ok(world.finish()?)
}
