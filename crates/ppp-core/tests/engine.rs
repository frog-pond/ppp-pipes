//! The sandboxed engine: evaluation, the embedded `ppp` module, and the
//! command set.

mod support;

use std::path::Path;

use nu_protocol::Value;
use ppp_core::engine::Engine;
use support::temp_dir;

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
    engine.eval("use ppp *", Value::test_nothing()).unwrap();
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
    for name in [
        "from ics",
        "from tribe-events",
        "from presence",
        "html to-text",
        "to pipes-json",
        "validate",
        "pipe get",
        "expand rrule",
        "into event",
        "drop-empty",
        "during",
        "uniq-events",
    ] {
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

#[test]
fn the_same_source_can_run_twice() {
    // nushell's parser caches blocks by source; a cached block came back
    // without its compiled form, so the second run failed.
    let mut engine = Engine::new();
    for i in 0..3 {
        let out = engine.eval("$in | to nuon", Value::test_int(i)).unwrap();
        assert_eq!(out.coerce_into_string().unwrap(), i.to_string());
    }
}

#[test]
fn modules_cannot_be_loaded_from_disk() {
    let dir = temp_dir("engine-disk-modules");
    // A file that isn't nushell: a parse error would quote it.
    let secret = dir.join("secret.nu");
    std::fs::write(&secret, "the secret is 4321\n").unwrap();
    // A file that is: loading it would run code from outside the pipe.
    let module = dir.join("module.nu");
    std::fs::write(&module, "export def leak [] { 'loaded' }\n").unwrap();
    let relative = |path: &Path| path.strip_prefix("/").unwrap().display().to_string();
    for path in [&secret, &module] {
        for source in [
            format!("use '{}'", path.display()),
            format!("use '{}'", relative(path)),
            format!("overlay use '{}'", path.display()),
            format!("module '{}'", path.display()),
        ] {
            let err = Engine::new()
                .eval(&source, Value::test_nothing())
                .expect_err(&source);
            assert!(!err.0.contains("4321"), "{source} showed the file:\n{err}");
            assert!(err.0.contains("pipes can't load files"), "{source}: {err}");
        }
    }
}
