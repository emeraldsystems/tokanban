use crate::api::{ApiClient, PaginatedResponse, ProjectItem};
use crate::config::AppConfig;
use crate::error::{CliError, Result};
use crate::format::{ColorConfig, OutputFormat};
use std::path::PathBuf;

/// Command execution context containing config, API client, and state.
pub struct Ctx {
    pub config: AppConfig,
    pub api: ApiClient,
    pub config_path: Option<PathBuf>,
    pub quiet: bool,
    pub verbose: bool,
    pub format: OutputFormat,
    pub color: ColorConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedProject {
    pub id: String,
    pub key: String,
    pub key_prefix: String,
    pub name: String,
}

impl Ctx {
    /// Create a new context from config.
    pub fn new(
        config: AppConfig,
        config_path: Option<PathBuf>,
        quiet: bool,
        verbose: bool,
        format: OutputFormat,
        no_color: bool,
    ) -> Result<Self> {
        let api = ApiClient::new(
            &config.api.url,
            config.api.timeout_secs,
            config.auth.access_token.clone(),
        )?;

        let color = ColorConfig::new(no_color);

        Ok(Self {
            config,
            api,
            config_path,
            quiet,
            verbose,
            format,
            color,
        })
    }

    /// Get workspace slug, from flag or config default.
    pub fn workspace_slug(&self, flag_value: Option<String>) -> Result<String> {
        if let Some(slug) = flag_value {
            return Ok(slug);
        }
        self.config
            .defaults
            .workspace
            .clone()
            .ok_or_else(|| crate::error::CliError::MissingRequired("workspace".into(), "workspace".into()))
    }

    /// Get the raw project reference from the flag or config default.
    pub fn project_key(&self, flag_value: Option<String>) -> Result<String> {
        if let Some(key) = flag_value {
            return Ok(key);
        }
        self.config
            .defaults
            .project
            .clone()
            .ok_or_else(|| crate::error::CliError::MissingRequired("project".into(), "project".into()))
    }

    /// Resolve a project reference from the flag or config default to the canonical backend ID.
    pub async fn project_id(&self, flag_value: Option<String>) -> Result<String> {
        Ok(self.project(flag_value).await?.id)
    }

    /// Resolve a project reference from the flag or config default.
    pub async fn project(&self, flag_value: Option<String>) -> Result<ResolvedProject> {
        let reference = self.project_key(flag_value)?;
        self.resolve_project(&reference).await
    }

    /// Resolve a project name, key prefix, or UUID to a canonical project record.
    pub async fn resolve_project(&self, reference: &str) -> Result<ResolvedProject> {
        let reference = reference.trim();
        if reference.is_empty() {
            return Err(CliError::InvalidInput(
                "Project reference cannot be empty.".to_string(),
            ));
        }

        let mut cursor: Option<String> = None;
        let mut casefold_key_match: Option<ResolvedProject> = None;
        let mut exact_name_matches: Vec<ResolvedProject> = Vec::new();
        let mut casefold_name_matches: Vec<ResolvedProject> = Vec::new();

        loop {
            let mut url = "/v1/projects?limit=100".to_string();
            if let Some(next_cursor) = &cursor {
                url.push_str("&cursor=");
                url.push_str(&encode_query_component(next_cursor));
            }

            let resp: PaginatedResponse<ProjectItem> = self.api.get(&url).await?;

            for item in resp.items {
                let project = ResolvedProject::from(item);

                if project.id == reference
                    || project.key == reference
                    || project.key_prefix == reference
                {
                    return Ok(project);
                }

                if project.key.eq_ignore_ascii_case(reference)
                    || project.key_prefix.eq_ignore_ascii_case(reference)
                {
                    casefold_key_match = Some(project.clone());
                }

                if project.name == reference {
                    exact_name_matches.push(project.clone());
                }

                if project.name.eq_ignore_ascii_case(reference) {
                    casefold_name_matches.push(project.clone());
                }
            }

            cursor = resp.cursor;
            if cursor.is_none() {
                break;
            }
        }

        if let Some(project) = casefold_key_match {
            return Ok(project);
        }

        match exact_name_matches.len() {
            1 => return Ok(exact_name_matches.remove(0)),
            n if n > 1 => {
                return Err(ambiguous_project_reference(reference, &exact_name_matches));
            }
            _ => {}
        }

        match casefold_name_matches.len() {
            1 => Ok(casefold_name_matches.remove(0)),
            n if n > 1 => Err(ambiguous_project_reference(reference, &casefold_name_matches)),
            _ => Err(CliError::InvalidInput(format!(
                "Project '{reference}' was not found. Use `tokanban project list` to see available projects."
            ))),
        }
    }
}

impl From<ProjectItem> for ResolvedProject {
    fn from(item: ProjectItem) -> Self {
        let key = if item.key.is_empty() {
            item.key_prefix.clone()
        } else {
            item.key.clone()
        };

        Self {
            id: item.id,
            key,
            key_prefix: item.key_prefix,
            name: item.name,
        }
    }
}

fn ambiguous_project_reference(reference: &str, matches: &[ResolvedProject]) -> CliError {
    let choices = matches
        .iter()
        .map(|project| format!("{} ({})", project.name, project.key))
        .collect::<Vec<_>>()
        .join(", ");

    CliError::InvalidInput(format!(
        "Project reference '{reference}' is ambiguous. Matches: {choices}. Use the project key prefix or ID."
    ))
}

fn encode_query_component(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}
