//! The sandboxed engine: evaluation, the embedded `pipes` module, and the
//! command set.

use std::path::Path;

use nu_protocol::Value;
use ppp_core::engine::Engine;

fn nuon(engine: &mut Engine, source: &str, input: Value) -> String {
    let out = engine.eval(source, input).unwrap();
    engine
        .eval("$in | to nuon", out)
        .unwrap()
        .coerce_into_string()
        .unwrap()
}

#[test]
fn evaluates_with_pipeline_input() {
    let mut engine = Engine::new();
    let out = nuon(
        &mut engine,
        "$in | lines | each { str uppercase }",
        Value::test_string("a\nb"),
    );
    assert_eq!(out, "[A, B]");
}

#[test]
fn definitions_persist_between_calls() {
    let mut engine = Engine::new();
    engine
        .eval("def twice [] { $in * 2 }", Value::test_nothing())
        .unwrap();
    assert_eq!(nuon(&mut engine, "21 | twice", Value::test_nothing()), "42");
}

#[test]
fn the_pipes_module_is_embedded() {
    let mut engine = Engine::new();
    engine.eval("use pipes *", Value::test_nothing()).unwrap();
}

#[test]
fn externals_and_filesystem_commands_are_not_registered() {
    let mut engine = Engine::new();
    for name in [
        "run-external",
        "open",
        "save",
        "cd",
        "rm",
        "ls",
        "mkdir",
        "cp",
        "mv",
        "source",
        "exec",
        "plugin add",
    ] {
        assert!(!engine.has_command(name), "{name} is registered");
    }
    for source in ["^ls", "open /etc/hosts", "'x' | save /tmp/ppp-sandbox-test"] {
        assert!(
            engine.eval(source, Value::test_nothing()).is_err(),
            "{source} ran"
        );
    }
    assert!(!Path::new("/tmp/ppp-sandbox-test").exists());
}

#[test]
fn our_commands_do_not_shadow_nushell_builtins() {
    let full = nu_command::add_shell_command_context(nu_cmd_lang::create_default_context());
    for name in ["from ics", "into event", "drop-empty", "during"] {
        assert!(
            full.find_decl(name.as_bytes(), &[]).is_none(),
            "nushell already has {name}"
        );
    }
}

#[test]
fn errors_are_rendered_with_their_source() {
    let mut engine = Engine::new();
    let err = engine
        .eval("[1 2] | each { $in + 'x' }", Value::test_nothing())
        .unwrap_err();
    assert!(err.0.contains("Error:"), "{err}");
    assert!(err.0.contains("$in + 'x'"), "{err}");
}

#[test]
fn the_working_directory_is_the_root() {
    let mut engine = Engine::new();
    assert_eq!(
        nuon(&mut engine, "$env.PWD", Value::test_nothing()),
        r#""/""#
    );
}

#[test]
fn no_registered_command_is_deprecated() {
    let mut engine = Engine::new();
    let deprecated = nuon(
        &mut engine,
        "scope commands | where type == built-in | where { $in.deprecation_info | where type == Command | is-not-empty } | get name",
        Value::test_nothing(),
    );
    assert_eq!(deprecated, "[]");
}

#[test]
fn parse_warnings_are_kept() {
    let mut engine = Engine::new();
    engine
        .eval(
            "@deprecated \"use new\"\ndef old [] { 1 }\nold",
            Value::test_nothing(),
        )
        .unwrap();
    let warnings = engine.take_warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("old is deprecated"), "{}", warnings[0]);
    assert!(engine.take_warnings().is_empty());
}
