use std::fs;
use std::path::PathBuf;

fn template(path: &str) -> String {
    let full_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    fs::read_to_string(&full_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", full_path.display()))
}

#[test]
fn codex_memory_block_covers_review_then_session_end_flow() {
    let body = template("templates/AGENTS.md.memory-block.md");

    assert!(body.contains("session_start"));
    assert!(body.contains("memory_relevant_now"));
    assert!(body.contains("tokanban memory candidate review"));
    assert!(body.contains("session_end_contract.learned"));
    assert!(body.contains("session_end("));
    assert!(body.contains("clear_after_session_end_ids"));
}

#[test]
fn claude_memory_block_covers_review_then_session_end_flow() {
    let body = template("templates/CLAUDE.md.memory-block.md");

    assert!(body.contains("session_start"));
    assert!(body.contains("memory_relevant_now"));
    assert!(body.contains("tokanban memory candidate review"));
    assert!(body.contains("session_end_contract.decisions_made"));
    assert!(body.contains("session_end"));
    assert!(body.contains("clear_after_session_end_ids"));
}
