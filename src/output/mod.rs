// Re-export the full formatting API from the format module so command
// handlers can use `crate::output::*` without needing to know the internals.
pub use crate::format::{
    ColorConfig,
    OutputFormat,
    TaskDetail,
    TaskSummary,
    EM_DASH,
    color_priority,
    color_status,
    format_priority,
    print_inline,
    print_json,
    print_pagination_footer,
    print_task_card,
    print_task_list,
    terminal_width,
    truncate,
};
pub use crate::format::inline::{
    bulk_summary,
    compact_card,
    mutation_archived,
    mutation_closed,
    mutation_created,
    mutation_deleted,
    mutation_invited,
    mutation_reopened,
    mutation_revoked,
    mutation_rotated,
    mutation_updated,
};
