//! Exercise the public, offline skills installer against isolated directories.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const NAMES: &[&str] = &[
    "tokanban",
    "tokanban-setup",
    "tokanban-memory",
    "tokanban-pm",
    "tokanban-architect",
    "tokanban-engineer",
    "tokanban-reviewer",
    "tokanban-researcher",
];

fn install(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tokanban"))
        .args(["--config"])
        .arg(root.join("account.toml"))
        .args(["--format", "json", "init", "--target-dir"])
        .arg(root.join("project with spaces"))
        .args(arguments)
        .output()
        .unwrap()
}

fn skills(root: &Path) -> std::path::PathBuf {
    root.join("project with spaces/.agents/skills")
}

fn report(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stderr)))
}

fn snapshot(root: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<std::path::PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[test]
fn preview_and_explicit_dry_run_leave_no_files_or_directories() {
    for extra in [vec![], vec!["--yes", "--dry-run"]] {
        let temp = tempfile::tempdir().unwrap();
        let mut args = vec!["--harness", "codex", "--skills-only"];
        args.extend(extra);
        let output = install(temp.path(), &args);
        assert!(output.status.success());
        let data = report(&output);
        assert_eq!(data["applied"], false);
        assert_eq!(data["steps"].as_array().unwrap().len(), 8);
        assert!(data["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["status"] == "would_create"));
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }
}

#[test]
fn installs_all_commands_and_references_without_touching_account_or_agents_file() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project with spaces");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("AGENTS.md"), "Keep my project instructions.\n").unwrap();
    // Skills-only mode must work even if an account cannot be parsed.
    fs::write(temp.path().join("account.toml"), "not valid = [toml").unwrap();
    let output = install(
        temp.path(),
        &["--harness", "codex", "--skills-only", "--yes"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(project.join("AGENTS.md")).unwrap(),
        "Keep my project instructions.\n"
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("account.toml")).unwrap(),
        "not valid = [toml"
    );
    let data = report(&output);
    assert_eq!(data["steps"].as_array().unwrap().len(), 8);
    for name in NAMES {
        let skill = skills(temp.path()).join(name);
        assert!(skill.join("SKILL.md").is_file());
        assert!(skill.join("agents/openai.yaml").is_file());
    }
    assert!(skills(temp.path())
        .join("tokanban/references/cli-quick-ref.md")
        .is_file());
    let files = snapshot(temp.path());
    assert_eq!(files.len(), 19); // 17 skill files plus the two preserved originals.
    let repeated = install(
        temp.path(),
        &["--harness", "codex", "--skills-only", "--yes"],
    );
    assert!(repeated.status.success());
    assert!(report(&repeated)["steps"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["status"] == "already_present"));
    assert_eq!(snapshot(temp.path()), files);
}

#[test]
fn preserves_a_custom_skill_and_does_not_partially_fill_it() {
    let temp = tempfile::tempdir().unwrap();
    let pm = skills(temp.path()).join("tokanban-pm");
    fs::create_dir_all(&pm).unwrap();
    fs::write(pm.join("SKILL.md"), "My custom PM\n").unwrap();
    let unrelated = skills(temp.path()).join("unrelated");
    fs::create_dir(&unrelated).unwrap();
    fs::write(unrelated.join("SKILL.md"), "Keep this skill\n").unwrap();
    let output = install(
        temp.path(),
        &["--harness", "codex", "--skills-only", "--yes"],
    );
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(pm.join("SKILL.md")).unwrap(),
        "My custom PM\n"
    );
    assert!(!pm.join("agents").exists());
    assert_eq!(
        fs::read_to_string(unrelated.join("SKILL.md")).unwrap(),
        "Keep this skill\n"
    );
    assert!(report(&output)["steps"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["status"] == "refused"));
    assert!(skills(temp.path())
        .join("tokanban-engineer/SKILL.md")
        .is_file());
}

#[test]
fn fills_missing_files_when_remaining_bundle_files_match() {
    let temp = tempfile::tempdir().unwrap();
    let args = &["--harness", "codex", "--skills-only", "--yes"];
    assert!(install(temp.path(), args).status.success());
    let expected = snapshot(temp.path());
    fs::remove_file(skills(temp.path()).join("tokanban/agents/openai.yaml")).unwrap();
    fs::remove_file(skills(temp.path()).join("tokanban/references/cli-quick-ref.md")).unwrap();
    assert!(install(temp.path(), args).status.success());
    assert_eq!(snapshot(temp.path()), expected);
}

#[test]
fn rejects_other_harnesses_and_requires_an_explicit_codex_selection() {
    for args in [
        vec!["--skills-only", "--yes"],
        vec!["--harness", "claude-code", "--skills-only", "--yes"],
        vec!["--harness", "cursor", "--skills-only", "--yes"],
    ] {
        let temp = tempfile::tempdir().unwrap();
        let output = install(temp.path(), &args);
        assert!(!output.status.success());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }
}

#[test]
fn refuses_a_file_where_the_skills_directory_should_be() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project with spaces");
    fs::create_dir(&project).unwrap();
    fs::write(project.join(".agents"), "preserve").unwrap();
    let before = snapshot(temp.path());
    let output = install(
        temp.path(),
        &["--harness", "codex", "--skills-only", "--yes"],
    );
    assert!(!output.status.success());
    assert_eq!(snapshot(temp.path()), before);
}

#[cfg(unix)]
#[test]
fn refuses_writes_through_a_symlinked_skills_parent() {
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let agents = skills(temp.path()).parent().unwrap().to_path_buf();
    fs::create_dir_all(&agents).unwrap();
    std::os::unix::fs::symlink(outside.path(), agents.join("skills")).unwrap();
    let output = install(
        temp.path(),
        &["--harness", "codex", "--skills-only", "--yes"],
    );
    assert!(!output.status.success());
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn accepts_identical_symlinked_bundles_but_never_repairs_through_them() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(skills(temp.path())).unwrap();
    let source = tempfile::tempdir().unwrap();
    let args = &["--harness", "codex", "--skills-only", "--yes"];
    assert!(install(source.path(), args).status.success());
    for name in NAMES {
        std::os::unix::fs::symlink(
            skills(source.path()).join(name),
            skills(temp.path()).join(name),
        )
        .unwrap();
    }
    let output = install(temp.path(), args);
    assert!(output.status.success());
    assert!(report(&output)["steps"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["status"] == "already_present"));
    let removed = skills(source.path()).join("tokanban-pm/agents/openai.yaml");
    fs::remove_file(&removed).unwrap();
    let output = install(temp.path(), args);
    assert!(!output.status.success());
    assert!(!removed.exists());
}
