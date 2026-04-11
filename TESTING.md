# Tokanban CLI Testing Guide

This document outlines the testing strategy, infrastructure, and test cases for the Tokanban CLI.

## Test Infrastructure

### Mock Server Setup
- **Location**: `tests/common/mock_server.rs`
- **Library**: `wiremock` crate
- **Purpose**: Mock HTTP API responses for integration testing without hitting the real API

### Test Utilities
- **Location**: `tests/common/mod.rs`
- **Utilities**:
  - `setup_temp_config()` - Create temporary config directory
  - `test_workspace()` - Generate test workspace slug
  - `test_project()` - Generate test project key
  - `test_user_id()` - Generate test user ID
  - `assertions` module - Error message assertions

### Test Fixtures
- **Location**: `tests/common/fixtures.rs`
- **Fixtures**:
  - `task_fixture()` - Sample task JSON
  - `project_fixture()` - Sample project JSON
  - `workspace_fixture()` - Sample workspace JSON
  - `user_fixture()` - Sample user JSON
  - `token_response()` - OAuth token response
  - `ConfigBuilder` - Programmatic config file builder

## Test Categories

### 1. Config Tests (Unit Tests)

**File**: `tests/config_tests.rs`

Tests for `src/auth/config.rs` configuration file handling.

#### Test Cases:
- ✅ `test_config_read_valid_file` - Read valid config.toml
- ✅ `test_config_write_all_fields` - Write config with all fields
- ✅ `test_config_defaults_resolution` - Apply defaults when fields missing
- ✅ `test_config_permission_check` - Reject file with mode > 0600
- ✅ `test_config_missing_file` - Handle missing config gracefully
- ✅ `test_config_invalid_toml` - Error on invalid TOML syntax
- ✅ `test_config_workspace_default` - Workspace default applied correctly
- ✅ `test_config_project_default` - Project default applied correctly

#### Blocked by: Task #2 (Config and auth system)

### 2. Auth Tests (Unit Tests)

**File**: `tests/auth_tests.rs`

Tests for `src/auth/mod.rs` and `src/auth/login.rs` authentication flows.

#### Test Cases:
- ✅ `test_pkce_challenge_generation` - Valid code_verifier and code_challenge
- ✅ `test_state_token_generation` - Random state tokens are unique
- ✅ `test_state_token_validation` - Stored and callback states match
- ✅ `test_token_refresh_check_expiry` - Check access token expiry at 60s threshold
- ✅ `test_token_refresh_exchange` - Exchange refresh token for access token
- ✅ `test_token_refresh_expired_token` - Handle 401 on expired refresh token
- ✅ `test_token_refresh_revoked_token` - Handle revoked token scenario
- ✅ `test_token_storage_in_config` - Tokens saved after auth success
- ✅ `test_token_silent_refresh` - Refresh happens silently before API call

#### Blocked by: Task #2 (Config and auth system)

### 3. Output Formatting Tests (Unit Tests)

**File**: `tests/output_tests.rs`

Tests for `src/output/formatter.rs`, `src/output/tui.rs`, and `src/output/json.rs`.

#### Test Cases:

##### Table Formatting:
- ✅ `test_table_list_format_headers` - Correct header row
- ✅ `test_table_alignment` - Columns aligned correctly
- ✅ `test_table_box_drawing_chars` - Uses box-drawing characters
- ✅ `test_table_truncation_with_ellipsis` - Long titles truncated with "..."
- ✅ `test_table_terminal_width` - Respects terminal width constraints
- ✅ `test_table_numeric_right_alignment` - Numbers and priorities right-aligned

##### Card Formatting:
- ✅ `test_card_header_format` - Card title with resource key
- ✅ `test_card_sections` - Fields organized in sections
- ✅ `test_card_box_drawing` - Uses box-drawing chars
- ✅ `test_card_comment_rendering` - Comments shown with mentions

##### JSON Output:
- ✅ `test_json_list_structure` - items, total, page, limit fields
- ✅ `test_json_detail_structure` - All fields serialized correctly
- ✅ `test_json_nested_objects` - Assignee, sprint as nested objects
- ✅ `test_json_timestamps` - ISO 8601 format

##### Format Auto-Detection:
- ✅ `test_format_explicit_flag_overrides` - --format flag takes precedence
- ✅ `test_format_quiet_uses_json` - --quiet flag switches to JSON
- ✅ `test_format_non_tty_uses_json` - Piped output uses JSON
- ✅ `test_format_tty_uses_tui` - Interactive terminal uses tables/cards
- ✅ `test_format_tty_detection_works` - isatty() detection functions

##### Color Handling:
- ✅ `test_color_codes_stripped_with_no_color` - --no-color removes ANSI codes
- ✅ `test_color_codes_not_piped` - No colors added when piping
- ✅ `test_color_codes_in_tty` - Colors applied in interactive terminal

##### Special Cases:
- ✅ `test_quiet_mode_no_output` - Only errors shown with --quiet
- ✅ `test_verbose_mode_includes_timing` - Response timing included
- ✅ `test_verbose_mode_includes_body` - Full response body shown

#### Blocked by: Task #3 (Output formatting system)

### 4. Command Parsing Tests (Unit Tests)

**File**: `tests/command_tests.rs`

Tests for `src/main.rs` and clap command parsing.

#### Test Cases:

##### Command Structure:
- ✅ `test_auth_login_parses` - `auth login` subcommand
- ✅ `test_auth_logout_parses` - `auth logout` subcommand
- ✅ `test_task_list_parses` - `task list` with all filters
- ✅ `test_task_create_parses` - `task create` with all options
- ✅ `test_project_list_parses` - `project list` subcommand
- ✅ `test_workspace_set_parses` - `workspace set` with slug

##### Required Arguments:
- ✅ `test_task_create_requires_title` - Error when title missing
- ✅ `test_task_create_requires_project` - Error when project missing
- ✅ `test_project_create_requires_name` - Error when name missing

##### Global Flags:
- ✅ `test_format_flag_all_values` - --format json, table, card accepted
- ✅ `test_workspace_flag_overrides` - --workspace overrides config
- ✅ `test_project_flag_overrides` - --project overrides config
- ✅ `test_quiet_flag_suppresses_output` - --quiet works
- ✅ `test_verbose_flag_included` - --verbose works
- ✅ `test_no_color_flag_works` - --no-color accepted
- ✅ `test_help_flag_generates_text` - -h and --help work
- ✅ `test_version_flag_works` - -v and --version work

##### Enum Case-Insensitivity:
- ✅ `test_format_json_lowercase` - --format json accepted
- ✅ `test_format_json_uppercase` - --format JSON accepted
- ✅ `test_status_enum_case_insensitive` - TODO, In Progress, etc.

##### Defaults from Config:
- ✅ `test_workspace_default_from_config` - Uses workspace from config
- ✅ `test_project_default_from_config` - Uses project from config
- ✅ `test_flag_overrides_config_default` - Flag takes precedence

#### Blocked by: Task #2 (Config) and #4 (Commands)

### 5. API Client Tests (Unit Tests)

**File**: `tests/api_tests.rs`

Tests for `src/api/client.rs` HTTP client behavior.

#### Test Cases:

##### Auth Headers:
- ✅ `test_auth_header_injected` - All requests include auth header
- ✅ `test_bearer_token_format` - "Bearer tk_access_..." format
- ✅ `test_auth_header_on_get` - GET requests have auth header
- ✅ `test_auth_header_on_post` - POST requests have auth header
- ✅ `test_auth_header_on_delete` - DELETE requests have auth header

##### Error Response Parsing:
- ✅ `test_error_type_extracted` - error field parsed
- ✅ `test_error_message_extracted` - message field parsed
- ✅ `test_error_hint_extracted` - hint field parsed
- ✅ `test_error_fix_extracted` - fix field parsed
- ✅ `test_error_unknown_type_handled` - Unknown error types don't panic

##### Timeout Handling:
- ✅ `test_request_timeout_error` - Timeout returns appropriate error
- ✅ `test_timeout_uses_config_value` - Uses timeout_secs from config
- ✅ `test_connection_refused_error` - Connection errors handled

##### Retry Logic:
- ✅ `test_transient_error_retried` - 5xx errors retried
- ✅ `test_permanent_error_not_retried` - 4xx errors not retried
- ✅ `test_max_retry_attempts` - Stops after max retries
- ✅ `test_exponential_backoff` - Backoff increases between retries

##### Status Code Handling:
- ✅ `test_200_success` - 200 OK parsed correctly
- ✅ `test_201_created` - 201 Created for mutations
- ✅ `test_400_bad_request` - Validation error message
- ✅ `test_401_unauthorized` - Auth error message
- ✅ `test_403_forbidden` - Permission denied message
- ✅ `test_404_not_found` - Resource not found message
- ✅ `test_429_rate_limit` - Rate limit error message
- ✅ `test_500_server_error` - Internal server error message

#### Blocked by: Task #2 (Auth) for token injection

### 6. Shell Completion Tests (Unit Tests)

**File**: `tests/completion_tests.rs`

Tests for `src/util/completion.rs` completion generation.

#### Test Cases:

##### Script Generation:
- ✅ `test_bash_completion_generated` - Bash script syntax valid
- ✅ `test_zsh_completion_generated` - Zsh script syntax valid
- ✅ `test_fish_completion_generated` - Fish script syntax valid

##### Static Completions:
- ✅ `test_command_names_in_completions` - auth, workspace, project, etc.
- ✅ `test_subcommand_names_in_completions` - login, logout, create, etc.
- ✅ `test_flag_names_in_completions` - --format, --quiet, etc.

##### Cache Behavior:
- ✅ `test_completion_cache_created` - ~/.cache/tokanban/completions dir created
- ✅ `test_completion_cache_ttl` - 5-minute TTL respected
- ✅ `test_completion_cache_miss_triggers_api_call` - API called on miss

#### Blocked by: Task #6 (Shell completions)

### 7. Integration Tests

**File**: `tests/integration_tests.rs`

Full end-to-end tests with mock API server.

#### Test Cases:

##### Task Commands:
- ✅ `test_task_list_json_output` - `task list --format json` returns correct JSON
- ✅ `test_task_list_table_output` - `task list` returns table in TTY
- ✅ `test_task_create_success` - `task create` creates and returns result
- ✅ `test_task_view_detail_output` - `task view` returns card format
- ✅ `test_task_update_success` - `task update` applies changes
- ✅ `test_task_search` - `task search` returns results

##### Workspace Commands:
- ✅ `test_workspace_set_updates_config` - `workspace set` updates config
- ✅ `test_workspace_list_displays_all` - `workspace list` shows all workspaces

##### Auth Flow:
- ✅ `test_auth_login_flow` - OAuth flow with mock server
- ✅ `test_auth_logout_clears_token` - `auth logout` removes token
- ✅ `test_auth_refresh_before_api_call` - Token refreshed if near expiry

##### Error Handling:
- ✅ `test_error_not_found_formatted` - 404 returns proper error message
- ✅ `test_error_unauthorized_formatted` - 401 returns auth error
- ✅ `test_error_rate_limit_formatted` - 429 returns rate limit error
- ✅ `test_error_exit_code_general` - Exit code 1 for general errors
- ✅ `test_error_exit_code_auth` - Exit code 2 for auth errors
- ✅ `test_error_exit_code_forbidden` - Exit code 3 for permission errors
- ✅ `test_error_exit_code_rate_limit` - Exit code 4 for transient errors

##### Output Behavior:
- ✅ `test_piped_output_is_json` - Non-TTY uses JSON automatically
- ✅ `test_tty_output_is_table` - TTY uses table format automatically
- ✅ `test_quiet_mode_no_mutations_output` - Mutations silent with --quiet
- ✅ `test_verbose_timing_included` - Response timing shown with --verbose

## Test Execution

### Run All Tests
```bash
cargo test
```

### Run Specific Test File
```bash
cargo test --test config_tests
cargo test --test output_tests
cargo test --test integration_tests
```

### Run With Output
```bash
cargo test -- --nocapture
```

### Run Single Test
```bash
cargo test test_config_read_valid_file
```

### Run With Logging
```bash
RUST_LOG=debug cargo test
```

## Test Dependencies

- `wiremock` - Mock HTTP server for integration tests
- `tempfile` - Temporary file/directory handling
- `assert_matches` - Pattern matching assertions
- `tokio-test` - Tokio runtime for async tests
- `tokio` - Async runtime (already in main dependencies)
- `serde_json` - JSON serialization in tests

## Test Coverage Goals

- **Unit Tests**: >80% of non-CLI logic
- **Integration Tests**: All command paths and error scenarios
- **Output Tests**: All format combinations (json, table, card, quiet, verbose, no-color, piped/TTY)

## Blocked Tests

The following test categories cannot be implemented until their blocker tasks complete:

| Test Category | Blocked By | Reason |
|---|---|---|
| Config | Task #2 | Need config module implementation |
| Auth | Task #2 | Need auth module implementation |
| Output | Task #3 | Need output formatting implementation |
| Commands | Tasks #2, #4 | Need auth and command implementations |
| Integration | Tasks #2, #3, #4 | All modules needed for full workflow |

## Manual Testing

Some scenarios cannot be fully automated:

1. **OAuth flow**: Requires actual browser interaction. Manual verification with test OAuth provider.
2. **Terminal detection**: Verify TTY auto-detection works in different shells.
3. **Color output**: Visual inspection that colors render correctly.
4. **Table truncation**: Visual verification with different terminal widths.

## Future Enhancements

- Property-based testing with `proptest` for randomized inputs
- Snapshot testing for output formats with `insta`
- Benchmarking for performance-sensitive paths with `criterion`
- Code coverage measurement with `tarpaulin`
