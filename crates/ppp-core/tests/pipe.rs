//! Loading pipe files, running them, and checking their records.

use std::path::{Path, PathBuf};

use nu_protocol::{Record, Value, record};
use ppp_core::pipe::{Param, Pipe, PipeError, check_records};
use ppp_core::schema::Registry;

fn pipe_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pipes")
        .join(name)
}

fn load(name: &str) -> Pipe {
    Pipe::load(&pipe_path(name)).unwrap()
}

fn load_error(name: &str) -> String {
    match Pipe::load(&pipe_path(name)) {
        Err(err) => err.to_string(),
        Ok(_) => panic!("{name} loaded"),
    }
}

fn summaries(records: &[Value]) -> Vec<String> {
    records
        .iter()
        .map(|r| {
            r.get_data_by_key("summary")
                .unwrap()
                .coerce_into_string()
                .unwrap()
        })
        .collect()
}

#[test]
fn reads_metadata_without_running_main() {
    let pipe = load("two-events.nu");
    let info = pipe.info();
    assert_eq!(info.name, "two-events");
    assert_eq!(info.title, "Two events");
    assert_eq!(info.description, "Both from an inline calendar.");
    assert_eq!(
        info.output,
        ["https://pipes.frogpond.tech/stdlib/calendar/1#event"]
    );
    assert_eq!(
        info.params,
        [
            Param {
                name: "count".into(),
                shape: "int".into(),
                description: "How many events to return".into(),
                default: Some(Value::test_int(2))
            },
            Param {
                name: "prefix".into(),
                shape: "string".into(),
                description: "Text put before each summary".into(),
                default: Some(Value::test_string(""))
            },
            Param {
                name: "loud".into(),
                shape: "switch".into(),
                description: "Shout the summaries".into(),
                default: None
            },
        ]
    );
}

#[test]
fn runs_with_defaults_and_with_flags() {
    let mut pipe = load("two-events.nu");
    assert_eq!(
        summaries(&pipe.run(&Record::new()).unwrap()),
        ["Chess Club", "Choir"]
    );
    let args = record! {
        "count" => Value::test_int(1),
        "prefix" => Value::test_string("Club: "),
        "loud" => Value::test_bool(true),
    };
    assert_eq!(summaries(&pipe.run(&args).unwrap()), ["CLUB: CHESS CLUB"]);
}

#[test]
fn a_false_switch_is_left_off() {
    let mut pipe = load("two-events.nu");
    let records = pipe
        .run(&record! { "loud" => Value::test_bool(false) })
        .unwrap();
    assert_eq!(summaries(&records), ["Chess Club", "Choir"]);
}

#[test]
fn unknown_flags_are_an_error() {
    let mut pipe = load("two-events.nu");
    let err = pipe
        .run(&record! { "days" => Value::test_int(3) })
        .unwrap_err();
    assert!(matches!(err, PipeError::UnknownFlag { ref flag, .. } if flag == "days"));
    assert_eq!(err.to_string(), "two-events has no flag --days");
}

#[test]
fn checked_records_are_json() {
    let registry = Registry::stdlib().unwrap();
    let mut pipe = load("two-events.nu");
    let records = pipe.run(&Record::new()).unwrap();
    let json = check_records(&registry, pipe.info(), &records).unwrap();
    assert_eq!(json[0]["start"], "2026-10-13T18:00:00");
    assert_eq!(json[0]["tz"], "America/Chicago");
}

#[test]
fn undeclared_record_types_fail_the_check() {
    let registry = Registry::stdlib().unwrap();
    let mut pipe = load("wrong-type.nu");
    let records = pipe.run(&Record::new()).unwrap();
    let errors = check_records(&registry, pipe.info(), &records).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].pointer, "/type");
    assert_eq!(
        errors[0].message,
        "type \"entry\" is not one of the pipe's output types: https://pipes.frogpond.tech/stdlib/calendar/1#event"
    );
}

#[test]
fn invalid_records_fail_the_check() {
    let registry = Registry::stdlib().unwrap();
    let mut pipe = load("invalid-event.nu");
    let records = pipe.run(&Record::new()).unwrap();
    let errors = check_records(&registry, pipe.info(), &records).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("\"start\" is a required property")),
        "{errors:#?}"
    );
}

#[test]
fn bad_pipe_files() {
    assert!(load_error("no-const.nu").ends_with("the file has no `export const pipe = {...}`"));
    assert!(
        load_error("positional.nu")
            .ends_with("`main` has a positional parameter `days`; pipe parameters must be flags")
    );
    assert!(load_error("missing.nu").contains("No such file"));
}

#[test]
fn a_single_record_is_one_record() {
    let mut pipe = load("one-record.nu");
    assert_eq!(summaries(&pipe.run(&Record::new()).unwrap()), ["Solo"]);
}

#[test]
fn paths_with_spaces_and_quotes_load() {
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join("a dir with 'single' and \"double\" quotes");
    std::fs::create_dir_all(&dir).unwrap();
    let copy = dir.join("two-events.nu");
    std::fs::copy(pipe_path("two-events.nu"), &copy).unwrap();
    let mut pipe = Pipe::load(&copy).unwrap();
    assert_eq!(pipe.run(&Record::new()).unwrap().len(), 2);
}
