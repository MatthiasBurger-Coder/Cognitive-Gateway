//! Read-only inspection of persisted, governed experience.
use super::{CliError, Options, checked};
use crate::{postgres_experience::PostgresExperienceStore, postgres_memory::PostgresMemoryStore};
use gateway_application::{
    experience_patterns::{PatternLimits, PatternReport, inspect_patterns},
    memory::MemoryApplication,
};
use gateway_domain::{ContextScopeId, UnixTimestamp};
use std::{env, fs, path::PathBuf, time::SystemTime};

pub(super) fn inspect_database(options: &Options) -> Result<PatternReport, CliError> {
    let scope = checked(
        ContextScopeId::new(options.required("scope")?),
        3,
        "INVALID_SCOPE",
    )?;
    let at = match options.get("at") {
        Some(raw) => UnixTimestamp::new(
            raw.parse::<i64>()
                .map_err(|_| CliError::new(3, "INVALID_TIMESTAMP", "--at requires Unix seconds"))?,
        ),
        None => UnixTimestamp::new(
            i64::try_from(
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map_err(|_| CliError::new(3, "INVALID_TIMESTAMP", "system clock is invalid"))?
                    .as_secs(),
            )
            .map_err(|_| CliError::new(3, "INVALID_TIMESTAMP", "system clock is invalid"))?,
        ),
    };
    let config = database_config()?;
    let memory = MemoryApplication::new(checked(
        PostgresMemoryStore::connect_config(&config),
        10,
        "DATABASE_ERROR",
    )?);
    let source = checked(
        PostgresExperienceStore::connect_config(&config, Default::default()),
        10,
        "DATABASE_ERROR",
    )?;
    checked(
        inspect_patterns(&memory, &source, &scope, at, PatternLimits::default()),
        10,
        "PATTERN_INSPECTION_ERROR",
    )
}

fn database_config() -> Result<postgres::Config, CliError> {
    let path = if let Some(path) = env::var_os("CG_POSTGRES_ENV_FILE") {
        PathBuf::from(path)
    } else {
        let root = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .ok_or_else(|| CliError::new(3, "DATABASE_CONFIG", "config home is unavailable"))?;
        root.join("cognitive-gateway/postgres.env")
    };
    let content = fs::read_to_string(path)
        .map_err(|error| CliError::new(3, "DATABASE_CONFIG", error.to_string()))?;
    let setting = |key: &str| {
        content
            .lines()
            .filter_map(|line| line.split_once('='))
            .find_map(|(name, value)| (name.trim() == key).then_some(value.trim()))
    };
    let password = setting("CG_POSTGRES_PASSWORD")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CliError::new(3, "DATABASE_CONFIG", "CG_POSTGRES_PASSWORD is missing"))?;
    let port = setting("CG_POSTGRES_PORT")
        .map(str::parse::<u16>)
        .transpose()
        .map_err(|_| CliError::new(3, "DATABASE_CONFIG", "CG_POSTGRES_PORT is invalid"))?
        .unwrap_or(55432);
    let mut config = postgres::Config::new();
    config
        .host("127.0.0.1")
        .port(port)
        .user("cognitive_gateway")
        .dbname("cognitive_gateway")
        .password(password);
    Ok(config)
}
