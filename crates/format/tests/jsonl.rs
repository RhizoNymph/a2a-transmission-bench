//! Framing: header, world sections and a trailer whose counts and digest
//! make a cut or edited file an error.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::Cursor;

use a2a_bench_format::files::{ExchangeRow, Exchanges, MessageRow, Messages, WorldOnly};
use a2a_bench_format::ids::WorldKey;
use a2a_bench_format::jsonl::{BasicHeader, FileReader, FileWriter, ReadError, WriteError};
use a2a_bench_format::version::{FORMAT, Format};

fn world(key: &str) -> WorldOnly {
    WorldOnly {
        key: WorldKey::new(key).unwrap(),
    }
}

fn write_messages(worlds: &[(&str, Vec<&str>)]) -> Vec<u8> {
    let header = BasicHeader::new::<Messages>(common::dataset());
    let mut writer = FileWriter::<Messages, _>::new(Vec::new(), &header).unwrap();
    for (key, texts) in worlds {
        writer.world(&world(key)).unwrap();
        for text in texts {
            writer
                .row(&MessageRow::Message(common::user(text)))
                .unwrap();
        }
    }
    writer.finish().unwrap().0
}

fn read_all(bytes: &[u8]) -> Result<Vec<(String, usize)>, ReadError> {
    let mut reader = FileReader::<Messages, _>::open(Cursor::new(bytes))?;
    let mut out = Vec::new();
    assert!(reader.trailer().is_none());
    while let Some(section) = reader.next_world()? {
        out.push((section.world.key.to_string(), section.rows.len()));
    }
    let trailer = reader
        .trailer()
        .expect("a checked trailer after the last world");
    assert_eq!(usize::try_from(trailer.worlds).unwrap(), out.len());
    Ok(out)
}

#[test]
fn sections_round_trip_in_order() {
    let bytes = write_messages(&[("w1", vec!["a", "b"]), ("w2", vec![]), ("w3", vec!["c"])]);
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(
        text.starts_with(
            r#"{"kind":"header","format":"a2a-bench/1","file":"messages","dataset":"fixture"}"#
        ),
        "{text}"
    );
    assert!(
        text.lines()
            .last()
            .unwrap()
            .starts_with(r#"{"kind":"trailer","worlds":3,"rows":3,"digest":""#)
    );
    assert_eq!(
        read_all(&bytes).unwrap(),
        vec![("w1".into(), 2), ("w2".into(), 0), ("w3".into(), 1)]
    );
}

#[test]
fn writing_is_deterministic() {
    let worlds = [("w1", vec!["a", "b"]), ("w2", vec!["c"])];
    assert_eq!(write_messages(&worlds), write_messages(&worlds));
}

#[test]
fn a_cut_file_is_truncated() {
    let bytes = write_messages(&[("w1", vec!["a", "b"])]);
    let text = String::from_utf8(bytes).unwrap();
    let cut: String = text.lines().take(3).map(|l| format!("{l}\n")).collect();
    assert!(matches!(
        read_all(cut.as_bytes()),
        Err(ReadError::Truncated)
    ));
    assert!(matches!(read_all(b""), Err(ReadError::MissingHeader)));
}

#[test]
fn an_edited_or_padded_file_is_refused() {
    let bytes = write_messages(&[("w1", vec!["a"]), ("w2", vec!["b"])]);
    let text = String::from_utf8(bytes).unwrap();
    let mut lines: Vec<&str> = text.lines().collect();
    lines.swap(1, 3);
    let swapped = lines.join("\n") + "\n";
    assert!(matches!(
        read_all(swapped.as_bytes()),
        Err(ReadError::Digest { .. })
    ));
    let dropped: String = text
        .lines()
        .enumerate()
        .filter(|(at, _)| *at != 2)
        .map(|(_, l)| format!("{l}\n"))
        .collect();
    assert!(matches!(
        read_all(dropped.as_bytes()),
        Err(ReadError::Counts { .. })
    ));
    let padded = text.clone() + text.lines().nth(2).unwrap() + "\n";
    assert!(matches!(
        read_all(padded.as_bytes()),
        Err(ReadError::AfterTrailer { .. })
    ));
}

#[test]
fn structure_errors() {
    let bytes = write_messages(&[("w1", vec!["a"])]);
    let text = String::from_utf8(bytes).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let no_world = format!("{}\n{}\n", lines[0], lines[2]);
    assert!(matches!(
        read_all(no_world.as_bytes()),
        Err(ReadError::RowOutsideWorld { line: 2 })
    ));
    let no_header = format!("{}\n", lines[1]);
    assert!(matches!(
        read_all(no_header.as_bytes()),
        Err(ReadError::NotHeader { .. })
    ));
    let twice = format!("{}\n{}\n{}\n{}\n", lines[0], lines[1], lines[1], lines[3]);
    assert!(matches!(
        read_all(twice.as_bytes()),
        Err(ReadError::DuplicateWorld { .. })
    ));
}

#[test]
fn file_kind_and_format_are_checked() {
    let bytes = write_messages(&[("w1", vec![])]);
    assert!(matches!(
        FileReader::<Exchanges, _>::open(Cursor::new(&bytes)),
        Err(ReadError::WrongFile {
            expected: "exchanges",
            ..
        })
    ));
    let text = String::from_utf8(bytes)
        .unwrap()
        .replacen("a2a-bench/1", "a2a-bench/2", 1);
    assert!(matches!(
        read_all(text.as_bytes()),
        Err(ReadError::WrongFormat { .. })
    ));
    let wrong = BasicHeader::new::<Exchanges>(common::dataset());
    assert!(matches!(
        FileWriter::<Messages, _>::new(Vec::new(), &wrong),
        Err(WriteError::WrongFile { .. })
    ));
    let mut writer = FileWriter::<Exchanges, _>::new(Vec::new(), &wrong).unwrap();
    let fixture = common::fixture();
    assert!(matches!(
        writer.row(&ExchangeRow::Exchange(fixture.exchanges[0].clone())),
        Err(WriteError::RowOutsideWorld)
    ));
}

#[test]
fn format_text() {
    assert_eq!(FORMAT.to_string(), "a2a-bench/1");
    assert_eq!(Format::parse("a2a-bench/12").unwrap().major(), 12);
    assert!(Format::parse("a2a-bench/01").is_err());
    assert!(Format::parse("other/1").is_err());
}
