//! CLI/App/shell共享工作目录查询；不读取或修改会话状态。
use anyhow::{bail, Result};
use moss_core::provider::{LaunchSource, Snapshot};
use moss_core::slot::Slot;
use moss_core::working_directory::{self as policy, Context, Error, ValidatedWorkingDirectory};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub fn current_context() -> Result<Context> {
    context_for_source(&LaunchSource::capture(std::env::var_os(
        "CLAUDE_CONFIG_DIR",
    )))
}

pub fn context_for_source(source: &LaunchSource) -> Result<Context> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(Error::Unavailable)?;
    let roots = match source {
        LaunchSource::Unknown => return Err(Error::Unavailable.into()),
        LaunchSource::Unset => vec![home.join(".claude")],
        LaunchSource::Explicit(path) => vec![PathBuf::from(path)],
    };
    Ok(Context::new(home, roots)?
        .with_config_file(moss_core::config::config_path())?
        .with_source_identity(source))
}

pub fn source(slot: &Slot) -> LaunchSource {
    slot.provider_started
        .as_deref()
        .and_then(Snapshot::from_json)
        .unwrap_or_default()
        .launch_source
}

pub fn for_slot(slot: &Slot) -> Result<ValidatedWorkingDirectory> {
    Ok(policy::check(
        Path::new(&slot.cwd),
        &context_for_source(&source(slot))?,
    )?)
}

pub fn code(error: &anyhow::Error) -> &'static str {
    error
        .downcast_ref::<Error>()
        .copied()
        .unwrap_or(Error::Unavailable)
        .code()
}

pub fn outcome(
    path: &Path,
    result: Result<ValidatedWorkingDirectory>,
    source_resolved: bool,
) -> serde_json::Value {
    match result {
        Ok(validated) => {
            serde_json::json!({"path":path,"ok":true,"canonical":validated.canonical(),"reason":null,"message":null,"source_resolved":source_resolved})
        }
        Err(error) => {
            let reason = error
                .downcast_ref::<Error>()
                .copied()
                .unwrap_or(Error::Unavailable);
            serde_json::json!({"path":path,"ok":false,"canonical":null,"reason":reason.code(),"message":reason.to_string(),"source_resolved":source_resolved})
        }
    }
}

pub struct PlanningChecks {
    deadline: Instant,
    cache: std::collections::HashMap<PathBuf, std::result::Result<(), Error>>,
    exhausted: bool,
}
impl PlanningChecks {
    pub fn new() -> Self {
        Self {
            deadline: Instant::now() + Duration::from_millis(200),
            cache: Default::default(),
            exhausted: false,
        }
    }
    pub fn check(&mut self, path: &Path) -> std::result::Result<(), Error> {
        if self.exhausted {
            return Err(Error::Unavailable);
        }
        if let Some(result) = self.cache.get(path) {
            return *result;
        }
        if self.cache.len() >= 64 || Instant::now() >= self.deadline {
            self.exhausted = true;
            return Err(Error::Unavailable);
        }
        let result = policy::probe_directory(
            path,
            self.deadline.saturating_duration_since(Instant::now()),
        );
        if result == Err(Error::Unavailable) {
            self.exhausted = true;
        }
        self.cache.insert(path.to_path_buf(), result);
        result
    }
}
impl Default for PlanningChecks {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Checks {
    deadline: Instant,
    cache: std::collections::HashMap<
        (Context, PathBuf, bool),
        std::result::Result<ValidatedWorkingDirectory, Error>,
    >,
    exhausted: bool,
}
impl Default for Checks {
    fn default() -> Self {
        Self::new()
    }
}
impl Checks {
    pub fn new() -> Self {
        Self {
            deadline: Instant::now() + Duration::from_millis(200),
            cache: Default::default(),
            exhausted: false,
        }
    }
    pub fn check(
        &mut self,
        path: &Path,
        context: Result<Context>,
        resolved: bool,
    ) -> std::result::Result<ValidatedWorkingDirectory, Error> {
        if self.exhausted {
            return Err(Error::Unavailable);
        }
        let context = context.map_err(|error| {
            error
                .downcast_ref::<Error>()
                .copied()
                .unwrap_or(Error::Unavailable)
        })?;
        let key = (context.clone(), path.to_path_buf(), resolved);
        if let Some(value) = self.cache.get(&key) {
            return value.clone();
        }
        if self.cache.len() >= 64 || Instant::now() >= self.deadline {
            self.exhausted = true;
            return Err(Error::Unavailable);
        }
        let result = policy::check_with_budget(
            path,
            &context,
            self.deadline.saturating_duration_since(Instant::now()),
        );
        if matches!(result, Err(Error::Unavailable)) {
            self.exhausted = true;
        }
        self.cache.insert(key, result.clone());
        result
    }
    fn result(
        &mut self,
        path: &Path,
        context: Result<Context>,
        resolved: bool,
    ) -> serde_json::Value {
        outcome(
            path,
            self.check(path, context, resolved)
                .map_err(anyhow::Error::from),
            resolved,
        )
    }
    pub fn session(&mut self, slot: &Slot) -> serde_json::Value {
        let source = source(slot);
        let resolved = source != LaunchSource::Unknown;
        // 基础检查不把Unknown变成启动授权；最终入口仍要求明确source。
        let context = context_for_source(if resolved {
            &source
        } else {
            &LaunchSource::Unset
        });
        let mut value = self.result(Path::new(&slot.cwd), context, resolved);
        value
            .as_object_mut()
            .expect("outcome object")
            .remove("path");
        value
    }
}

pub fn run(paths: Vec<PathBuf>, json: bool) -> Result<()> {
    if paths.is_empty() || paths.len() > 64 || (!json && paths.len() != 1) {
        bail!("check-directory requires one path, or up to 64 paths with --json");
    }
    let mut checks = Checks::new();
    let mut results = Vec::new();
    for path in paths {
        let value = checks.result(&path, current_context(), true);
        if !json && value["ok"] != true {
            bail!(
                "{}",
                value["message"]
                    .as_str()
                    .unwrap_or("working directory check failed")
            );
        }
        if json {
            results.push(value);
        }
    }
    if json {
        println!("{}", serde_json::json!({"schema":1,"results":results}));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn working_directory_batch_uses_context_path_cache_and_total_limit() {
        let home = tempfile::tempdir().unwrap();
        let context = Context::new(home.path().to_path_buf(), vec![]).unwrap();
        let mut checks = Checks::new();
        checks.deadline = Instant::now() + Duration::from_secs(2);
        for _ in 0..3 {
            let _ = checks.result(Path::new("/"), Ok(context.clone()), true);
        }
        assert_eq!(checks.cache.len(), 1);
        for n in 0..63 {
            let path = PathBuf::from(format!("/bin/blocked-{n}"));
            let _ = checks.result(&path, Ok(context.clone()), true);
        }
        assert_eq!(checks.cache.len(), 64);
        assert_eq!(
            checks.result(Path::new("/new-probe"), Ok(context), true)["reason"],
            "cwd_unavailable"
        );
        assert!(checks.exhausted);
    }
    #[test]
    fn working_directory_batch_separates_unknown_scope_and_launch_context() {
        let home = tempfile::tempdir().unwrap();
        let context = Context::new(home.path().to_path_buf(), vec![]).unwrap();
        let mut checks = Checks::new();
        checks.deadline = Instant::now() + Duration::from_secs(2);
        let _ = checks.result(Path::new("/"), Ok(context.clone()), false);
        let _ = checks.result(Path::new("/"), Ok(context.clone()), true);
        let _ = checks.result(
            Path::new("/"),
            Ok(context.clone().with_source_identity(&LaunchSource::Unset)),
            true,
        );
        let _ = checks.result(
            Path::new("/"),
            Ok(context
                .clone()
                .with_source_identity(&LaunchSource::Explicit(
                    home.path().to_string_lossy().into_owned(),
                ))),
            true,
        );
        let _ = checks.result(
            Path::new("/"),
            Ok(context
                .with_config_file(home.path().join("config.toml"))
                .unwrap()),
            true,
        );
        assert_eq!(checks.cache.len(), 5);
    }
    #[test]
    fn working_directory_planning_preserves_typed_errors_and_shared_deadline() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("file");
        std::fs::write(&file, "x").unwrap();
        let mut checks = PlanningChecks::new();
        checks.deadline = Instant::now() + Duration::from_secs(2);
        assert_eq!(checks.check(&file), Err(Error::NotDirectory));
        assert_eq!(
            checks.check(&temp.path().join("missing")),
            Err(Error::Missing)
        );
        checks.deadline = Instant::now();
        assert_eq!(checks.check(temp.path()), Err(Error::Unavailable));
        assert_eq!(checks.check(&file), Err(Error::Unavailable));
    }
    #[test]
    fn working_directory_batch_expired_deadline_does_not_probe() {
        let home = tempfile::tempdir().unwrap();
        let context = Context::new(home.path().to_path_buf(), vec![]).unwrap();
        let mut checks = Checks::new();
        checks.deadline = Instant::now();
        assert_eq!(
            checks.result(Path::new("/"), Ok(context), true)["reason"],
            "cwd_unavailable"
        );
        assert!(checks.cache.is_empty());
    }
}
