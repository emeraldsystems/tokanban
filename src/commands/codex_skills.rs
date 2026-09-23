//! Offline installation of the Codex skills shipped inside the CLI binary.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use crate::error::Result;

use super::init::{InitStep, StepStatus};

struct Skill {
    name: &'static str,
    files: &'static [(&'static str, &'static str)],
}

macro_rules! skill {
    ($name:literal $(, $extra:literal)*) => {
        Skill {
            name: $name,
            files: &[
                ("SKILL.md", include_str!(concat!("../../codex/skills/", $name, "/SKILL.md"))),
                ("agents/openai.yaml", include_str!(concat!("../../codex/skills/", $name, "/agents/openai.yaml"))),
                $(($extra, include_str!(concat!("../../codex/skills/", $name, "/", $extra))),)*
            ],
        }
    };
}

const SKILLS: &[Skill] = &[
    skill!("tokanban-pm"),
    skill!("tokanban-architect"),
    skill!("tokanban-engineer"),
    skill!("tokanban-reviewer"),
    skill!("tokanban-researcher"),
    skill!("tokanban", "references/cli-quick-ref.md"),
    skill!("tokanban-setup"),
    skill!("tokanban-memory"),
];

pub(super) fn install(target: &Path, apply: bool) -> Result<Vec<InitStep>> {
    SKILLS
        .iter()
        .map(|skill| install_skill(target, skill, apply))
        .collect()
}

fn refused(name: String, detail: String) -> InitStep {
    InitStep {
        name,
        status: StepStatus::Refused,
        detail,
    }
}

fn install_skill(target: &Path, skill: &Skill, apply: bool) -> Result<InitStep> {
    let name = format!("Codex skill (${})", skill.name);
    let relative_root = Path::new(".agents").join("skills").join(skill.name);
    let root = target.join(&relative_root);
    let existed = root.exists();
    let mut missing = Vec::new();
    let mut linked = false;

    // Preflight the whole skill before writing any of its files. Existing
    // identical symlinked skills are supported, but never write through a link.
    for (relative, contents) in skill.files {
        let relative_path = relative_root.join(relative);
        let mut parent = target.to_path_buf();
        for component in relative_path.parent().unwrap().components() {
            parent.push(component);
            match fs::symlink_metadata(&parent) {
                Ok(metadata) => {
                    linked |= metadata.file_type().is_symlink();
                    if !parent.is_dir() {
                        return Ok(refused(
                            name,
                            format!(
                                "{} is not a directory. Existing paths were preserved.",
                                parent.display()
                            ),
                        ));
                    }
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        let path = target.join(relative_path);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                linked |= metadata.file_type().is_symlink();
                if !path.is_file() || fs::read(&path)? != contents.as_bytes() {
                    return Ok(refused(name, format!(
                        "{} differs from the bundled skill or is not a regular file. Preserved the entire existing skill; review or move it before reinstalling.", path.display()
                    )));
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                missing.push((path, contents.as_bytes()));
            }
            Err(error) => return Err(error.into()),
        }
    }

    if missing.is_empty() {
        return Ok(InitStep {
            name,
            status: StepStatus::AlreadyPresent,
            detail: format!("{} matches the bundled skill.", root.display()),
        });
    }
    if linked {
        return Ok(refused(name, format!(
            "{} has symlinked paths and missing files. Preserved it without writing through links; install into a regular directory or update the linked source.", root.display()
        )));
    }
    if apply {
        for (path, contents) in &missing {
            write_new(path, contents)?;
        }
    }

    Ok(InitStep {
        name,
        status: match (apply, existed) {
            (true, false) => StepStatus::Created,
            (true, true) => StepStatus::Updated,
            (false, false) => StepStatus::WouldCreate,
            (false, true) => StepStatus::WouldUpdate,
        },
        detail: format!(
            "Install {} missing file(s) into {}. Invoke with ${}; restart Codex if it does not appear.",
            missing.len(), root.display(), skill.name
        ),
    })
}

fn write_new(path: &PathBuf, contents: &[u8]) -> Result<()> {
    fs::create_dir_all(path.parent().unwrap())?;
    // create_new prevents a concurrent installer from overwriting a file that
    // appeared after preflight. Remove only a file this invocation created.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    if let Err(error) = file.write_all(contents).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(error.into());
    }
    Ok(())
}
