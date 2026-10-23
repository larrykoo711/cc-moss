//! `cc-moss restore`: bring interrupted sessions back after a crash or reboot.

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Result};
use moss_core::config::Config;
use moss_core::event::TmuxLoc;
use moss_core::registry::Registry;
use moss_core::restore::{plan, Verdict};
use moss_core::slot::{now_ms, Slot, Status};
use moss_core::tmux::{ProcessRunner, Socket, Tmux};
use moss_core::{naming, proc, store_dir};

use super::confirm;
use super::ls::JSON_SCHEMA;
use super::restart::{RestoreConfigSource, ResumePreflightError};

pub struct RestoreArgs {
    pub config_source: RestoreConfigSource,
    pub require_unknown_config: bool,
    pub expected_source_revision: Option<String>,
    pub provider_default: bool,
    pub yes: bool,
    pub dry_run: bool,
    /// Print one result per session as JSON instead of text (for the app).
    pub json: bool,
    pub names: Vec<String>,
}

/// Why a session did not come back, as a stable code for `--json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Killed before the first message: nothing to resume.
    Empty,
    /// Claude Code exited right after it started.
    ExitedAtOnce,
    /// tmux could not start (or reach) the session.
    TmuxFailed,
    ConfigSourceUnknown,
    ConfigSourceChanged,
    ConfigSourceUnsupported,
    InvalidConfigSource,
    ConfigDirectoryMissing,
    ConfigNotDirectory,
    ConfigSourceUnreadable,
    ConfigSourceUnavailable,
    ConfigSettingsInvalid,
    ResumePreflightFailed,
    WorkingDirectory(moss_core::working_directory::Error),
    /// It is running, on its way back, or comes back through another copy.
    AlreadyRunning,
    /// The name matches several sessions (`--json` only).
    Ambiguous,
    /// The name matches no session (`--json` only).
    NotFound,
}

impl Reason {
    pub fn code(self) -> &'static str {
        match self {
            Reason::Empty => "empty",
            Reason::ExitedAtOnce => "exited_at_once",
            Reason::TmuxFailed => "tmux_failed",
            Reason::ConfigSourceUnknown => "config_source_unknown",
            Reason::ConfigSourceChanged => "config_source_changed",
            Reason::ConfigSourceUnsupported => "config_source_unsupported",
            Reason::InvalidConfigSource => "invalid_config_source",
            Reason::ConfigDirectoryMissing => "config_directory_missing",
            Reason::ConfigNotDirectory => "config_not_directory",
            Reason::ConfigSourceUnreadable => "config_source_unreadable",
            Reason::ConfigSourceUnavailable => "config_source_unavailable",
            Reason::ConfigSettingsInvalid => "config_settings_invalid",
            Reason::ResumePreflightFailed => "resume_preflight_failed",
            Reason::WorkingDirectory(error) => error.code(),
            Reason::AlreadyRunning => "already_running",
            Reason::Ambiguous => "ambiguous",
            Reason::NotFound => "not_found",
        }
    }
}

/// One line of `restore --json`.
struct Outcome {
    id: String,
    name: String,
    failure: Option<(Reason, String)>,
    /// For `ambiguous`: the id prefixes of the sessions the name matches.
    candidates: Vec<String>,
    result: Option<&'static str>,
}

impl Outcome {
    fn to_json(&self) -> serde_json::Value {
        let mut v = serde_json::json!({
            "id": self.id,
            "name": self.name,
            "ok": self.failure.is_none(),
        });
        if let Some(result) = self.result {
            v["result"] = result.into();
        }
        if let Some((reason, message)) = &self.failure {
            v["reason"] = reason.code().into();
            v["message"] = message.as_str().into();
        }
        if !self.candidates.is_empty() {
            v["candidates"] = self.candidates.clone().into();
        }
        v
    }
}

fn print_json(outcomes: &[Outcome]) -> Result<()> {
    let results: Vec<_> = outcomes.iter().map(Outcome::to_json).collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": JSON_SCHEMA,
            "results": results,
        }))?
    );
    Ok(())
}

pub fn run(args: RestoreArgs) -> Result<()> {
    run_selected(args, None)
}

pub fn run_exact(args: RestoreArgs, slot_id: &str) -> Result<()> {
    run_selected(args, Some(slot_id))
}

fn exact_missing(args: &RestoreArgs, slot_id: &str) -> Result<()> {
    let message = "the selected slot or current session identity changed; no session was started";
    if args.json {
        return print_json(&[Outcome {
            id: slot_id.into(),
            name: args.names[0].clone(),
            failure: Some((Reason::NotFound, message.into())),
            candidates: Vec::new(),
            result: None,
        }]);
    }
    bail!(message)
}

fn run_selected(args: RestoreArgs, exact_slot_id: Option<&str>) -> Result<()> {
    if exact_slot_id.is_some()
        && (args.names.len() != 1
            || args.names[0].len() != 36
            || uuid::Uuid::parse_str(&args.names[0]).is_err())
    {
        bail!("exact restore requires one complete session UUID");
    }
    if (args.require_unknown_config || args.expected_source_revision.is_some())
        && matches!(args.config_source, RestoreConfigSource::Recorded)
    {
        bail!("--require-unknown-config requires an explicit config source choice");
    }
    if !matches!(args.config_source, RestoreConfigSource::Recorded) {
        if args.names.len() != 1 {
            bail!("config source selection requires exactly one session target");
        }
        if args.provider_default {
            bail!("config source selection cannot be combined with --provider-default");
        }
    }
    let config = Config::load()?;
    let path = store_dir::registry_path();
    if !path.exists() {
        if let Some(slot_id) = exact_slot_id {
            return exact_missing(&args, slot_id);
        }
        if args.json {
            return print_json(&[]);
        }
        println!("{}", nothing_to_restore());
        return Ok(());
    }
    let registry = Registry::open(&path)?;
    let slots = registry.slots()?;
    // Names select exact slots (by id), whatever form the user typed.
    // With --json a name that picks no one session is a result of its own.
    let mut unresolved = Vec::new();
    let only: Option<HashSet<String>> = if let Some(slot_id) = exact_slot_id {
        let Some(slot) = slots
            .iter()
            .find(|slot| slot.slot_id == slot_id && slot.session_id == args.names[0])
        else {
            return exact_missing(&args, slot_id);
        };
        Some([slot.slot_id.clone()].into())
    } else if args.names.is_empty() {
        None
    } else if args.json {
        let mut ids = HashSet::new();
        for n in &args.names {
            match super::resolve_item(&slots, n) {
                Ok(s) => {
                    ids.insert(s.slot_id.clone());
                }
                Err(u) => unresolved.push(Outcome {
                    id: String::new(),
                    name: n.clone(),
                    failure: Some((
                        if u.ambiguous {
                            Reason::Ambiguous
                        } else {
                            Reason::NotFound
                        },
                        u.message,
                    )),
                    candidates: u.candidates,
                    result: None,
                }),
            }
        }
        Some(ids)
    } else {
        Some(
            args.names
                .iter()
                .map(|n| Ok(super::resolve(&slots, n)?.slot_id.clone()))
                .collect::<Result<_>>()?,
        )
    };
    let rows: Vec<_> = slots
        .into_iter()
        .map(|s| (s.effective_status(proc::is_alive), s))
        .collect();
    // Archived sessions, and ones the user ended on purpose (shown again
    // after the archive), come back only when named.
    let pool = only.clone().or_else(|| {
        Some(
            rows.iter()
                .filter(|(_, s)| !s.is_archived() && !s.ended_on_purpose())
                .map(|(_, s)| s.slot_id.clone())
                .collect(),
        )
    });
    let mut cwd_probe = super::working_directory::PlanningChecks::new();
    let mut cwd_results = std::collections::HashMap::new();
    let mut cwd_errors = std::collections::HashMap::new();
    for (status, slot) in &rows {
        if *status != Status::Interrupted
            || slot.is_moss()
            || pool
                .as_ref()
                .is_some_and(|ids| !ids.contains(&slot.slot_id))
        {
            continue;
        }
        let result = cwd_probe.check(Path::new(&slot.cwd));
        cwd_results.insert(Path::new(&slot.cwd), result);
        if let Err(error) = result {
            cwd_errors.insert(slot.slot_id.clone(), error);
        }
    }
    // 旧plan只使用可恢复性；完整typed错误一直保留，返回时绝不由bool猜成missing。
    let candidates = plan(&rows, pool.as_ref(), now_ms(), |p: &Path| {
        cwd_results.get(p).map_or_else(|| p.exists(), Result::is_ok)
    });
    if args.json {
        let running: Vec<_> = rows
            .iter()
            .filter(|(status, slot)| {
                *status == Status::Running
                    && only.as_ref().is_some_and(|ids| ids.contains(&slot.slot_id))
            })
            .map(|(_, slot)| slot)
            .collect();
        return run_json(
            &config,
            &registry,
            &running,
            &candidates,
            unresolved,
            &args,
            &cwd_errors,
        );
    }
    if candidates.is_empty() {
        println!("{}", nothing_to_restore());
        return Ok(());
    }

    for c in &candidates {
        let why = cwd_errors
            .get(&c.slot.slot_id)
            .map(ToString::to_string)
            .unwrap_or_else(|| verdict_text(c.verdict).to_string());
        let note = c
            .slot
            .notes
            .state
            .as_deref()
            .or(c.slot.notes.note.as_deref());
        println!(
            "{} {} {}{}",
            super::pad(&display_name(c.slot), 24),
            super::pad(&why, 52),
            c.slot.cwd,
            note.map(|n| format!("  [{n}]")).unwrap_or_default()
        );
    }
    let todo: Vec<&Slot> = candidates
        .iter()
        .filter(|c| c.verdict == Verdict::Restore && !cwd_errors.contains_key(&c.slot.slot_id))
        .map(|c| c.slot)
        .collect();
    if todo.is_empty() {
        println!(
            "{}",
            tr!("Nothing here can be restored.", "这些会话都恢复不了。")
        );
        return Ok(());
    }
    if args.dry_run {
        println!(
            "{}",
            tr!(
                format!("Dry run: would restore {} session(s).", todo.len()),
                format!("演练模式：会恢复 {} 个会话。", todo.len())
            )
        );
        return Ok(());
    }
    let question = tr!(
        format!("Bring back {} session(s)?", todo.len()),
        format!("要把这 {} 个会话恢复回来吗？", todo.len())
    );
    let refused = tr!(
        "re-run with --yes to restore them",
        "加上 --yes 重新运行即可恢复"
    );
    if !confirm::ask(args.yes, &question, refused)? {
        println!("{}", tr!("Okay, nothing restored.", "好的，什么都没动。"));
        return Ok(());
    }

    let mut launcher = Launcher::new(&config)?
        .provider_default(args.provider_default)
        .config_source(args.config_source, args.require_unknown_config)
        .expected_source_revision(args.expected_source_revision);
    let mut failed = 0;
    for slot in todo {
        match launcher.launch(&registry, slot)? {
            Launched::Started(_) => {}
            Launched::Changed => println!(
                "{}",
                tr!(
                    format!(
                        "Skipped {}: it changed since the list above (resumed elsewhere?)",
                        display_name(slot)
                    ),
                    format!(
                        "跳过 {}：列出之后它有了变化（可能已在别处恢复？）",
                        display_name(slot)
                    )
                )
            ),
            Launched::Failed(..) => failed += 1,
        }
    }
    println!(
        "{}",
        tr!(
            "Jump back in with `t`, or `cc-moss attach <name>`.",
            "敲 `t` 或 `cc-moss attach <名字>` 回到会话里。"
        )
    );
    if failed > 0 {
        bail!(tr!(
            format!("{failed} session(s) could not be restored"),
            format!("{failed} 个会话没能恢复")
        ));
    }
    Ok(())
}

fn verdict_text(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Restore => tr!("restore", "恢复"),
        Verdict::EmptySession => {
            tr!("skip: no messages were ever sent", "跳过：一句话都没发过")
        }
        Verdict::MissingCwd => tr!("skip: its folder is gone", "跳过：目录已经不在了"),
        Verdict::RestorePending => tr!("skip: already on its way back", "跳过：已经在恢复中"),
        Verdict::OpenElsewhere => tr!(
            "skip: this conversation is open in another session",
            "跳过：这段对话正在别处打开着"
        ),
        Verdict::Duplicate => tr!(
            "skip: a newer copy of this conversation is restored instead",
            "跳过：会恢复这段对话更新的那份"
        ),
    }
}

/// `restore --yes --json`: restore every restorable candidate without asking
/// and report each session, including the ones named but already running.
/// Failures are results, not an error: the JSON is always printed.
fn run_json(
    config: &Config,
    registry: &Registry,
    running: &[&Slot],
    candidates: &[moss_core::restore::Candidate<'_>],
    unresolved: Vec<Outcome>,
    args: &RestoreArgs,
    cwd_errors: &std::collections::HashMap<String, moss_core::working_directory::Error>,
) -> Result<()> {
    let mut outcomes = unresolved;
    for slot in running {
        outcomes.push(Outcome {
            id: slot.slot_id.clone(),
            name: display_name(slot),
            failure: Some((
                Reason::AlreadyRunning,
                tr!("it is already running", "它已经在运行了").to_string(),
            )),
            candidates: Vec::new(),
            result: None,
        });
    }
    let mut launcher: Option<Launcher> = None;
    for c in candidates {
        let reason = match c.verdict {
            Verdict::Restore => cwd_errors
                .get(&c.slot.slot_id)
                .copied()
                .map(Reason::WorkingDirectory),
            Verdict::EmptySession => Some(Reason::Empty),
            Verdict::MissingCwd => Some(Reason::WorkingDirectory(
                *cwd_errors
                    .get(&c.slot.slot_id)
                    .expect("cwd rejection retains its typed error"),
            )),
            Verdict::RestorePending | Verdict::OpenElsewhere | Verdict::Duplicate => {
                Some(Reason::AlreadyRunning)
            }
        };
        let failure = match reason {
            Some(r) => Some((
                r,
                match r {
                    Reason::WorkingDirectory(error) => error.to_string(),
                    _ => verdict_text(c.verdict).to_string(),
                },
            )),
            None => {
                if launcher.is_none() {
                    launcher = Some(
                        Launcher::new(config)?
                            .provider_default(args.provider_default)
                            .config_source(args.config_source.clone(), args.require_unknown_config)
                            .expected_source_revision(args.expected_source_revision.clone())
                            .quiet(),
                    );
                }
                let l = launcher.as_mut().expect("just created");
                match l.launch(registry, c.slot)? {
                    Launched::Started(_) => None,
                    Launched::Changed => Some((
                        Reason::AlreadyRunning,
                        tr!(
                            "it changed meanwhile (resumed elsewhere?)",
                            "它刚刚有了变化（可能已在别处恢复？）"
                        )
                        .to_string(),
                    )),
                    Launched::Failed(r, message) => Some((r, message)),
                }
            }
        };
        let result = if failure.is_none() {
            let confirmed = registry.slots()?.iter().any(|s| {
                s.slot_id == c.slot.slot_id
                    && s.restore_started_at_ms.is_none()
                    && s.effective_status(proc::is_alive) == Status::Running
            });
            Some(if confirmed { "restored" } else { "starting" })
        } else {
            None
        };
        outcomes.push(Outcome {
            id: c.slot.slot_id.clone(),
            name: display_name(c.slot),
            failure,
            candidates: Vec::new(),
            result,
        });
    }
    print_json(&outcomes)
}

pub enum Launched {
    /// Running in this tmux session.
    Started(String),
    /// Some hook touched the slot after planning; left alone.
    Changed,
    /// Did not come back: why, and the words already printed to stderr.
    Failed(Reason, String),
}

fn source_is_unknown(slot: &Slot) -> bool {
    slot.provider_started
        .as_deref()
        .and_then(moss_core::provider::Snapshot::from_json)
        .unwrap_or_default()
        .launch_source
        == moss_core::provider::LaunchSource::Unknown
}

fn preflight_failure(error: anyhow::Error) -> Launched {
    if let Some(error) = error.downcast_ref::<moss_core::working_directory::Error>() {
        return Launched::Failed(Reason::WorkingDirectory(*error), error.to_string());
    }
    if let Some(issue) = error.downcast_ref::<super::restore_source::Issue>() {
        use super::restore_source::Issue;
        let reason = match issue {
            Issue::Unknown => Reason::ConfigSourceUnknown,
            Issue::Unsupported => Reason::ConfigSourceUnsupported,
            Issue::InvalidPath => Reason::InvalidConfigSource,
            Issue::MissingDirectory => Reason::ConfigDirectoryMissing,
            Issue::NotDirectory => Reason::ConfigNotDirectory,
            Issue::Unreadable => Reason::ConfigSourceUnreadable,
            Issue::Unavailable => Reason::ConfigSourceUnavailable,
            Issue::InvalidSettings => Reason::ConfigSettingsInvalid,
        };
        return Launched::Failed(reason, issue.to_string());
    }
    let reason = match error.downcast_ref::<ResumePreflightError>() {
        Some(ResumePreflightError::ConfigSourceUnknown) => Reason::ConfigSourceUnknown,
        Some(ResumePreflightError::ConfigSourceChanged) => Reason::ConfigSourceChanged,
        Some(ResumePreflightError::ConfigSourceUnsupported) => Reason::ConfigSourceUnsupported,
        Some(ResumePreflightError::InvalidConfigSource) => Reason::InvalidConfigSource,
        None => Reason::ResumePreflightFailed,
    };
    Launched::Failed(reason, format!("{error:#}"))
}

#[derive(Debug, PartialEq, Eq)]
enum SourceClaim {
    Claimed,
    Changed,
    SourceChanged,
}

fn claim_with_source_guard(
    registry: &Registry,
    slot: &Slot,
    require_unknown: bool,
) -> Result<SourceClaim> {
    if require_unknown && !source_is_unknown(slot) {
        return Ok(SourceClaim::SourceChanged);
    }
    if registry.claim_restore(slot, now_ms())? {
        return Ok(SourceClaim::Claimed);
    }
    if require_unknown
        && registry.slots()?.iter().any(|current| {
            current.slot_id == slot.slot_id
                && current.session_id == slot.session_id
                && !source_is_unknown(current)
        })
    {
        return Ok(SourceClaim::SourceChanged);
    }
    Ok(SourceClaim::Changed)
}

pub(super) fn launcher_home<'a>(
    source_home: Option<&Path>,
    working: &'a moss_core::working_directory::ValidatedWorkingDirectory,
) -> std::result::Result<&'a Path, moss_core::working_directory::Error> {
    if source_home.is_some_and(|home| home != working.target_home()) {
        return Err(moss_core::working_directory::Error::Changed);
    }
    Ok(working.target_home())
}

/// Relaunches interrupted slots in tmux: claim, start, record the process.
pub struct Launcher {
    tmux: Tmux<ProcessRunner>,
    prefix: Option<String>,
    taken: Vec<String>,
    claude: String,
    /// Keep stdout clean (for `--json`): no "Started …" lines.
    quiet: bool,
    provider_default: bool,
    config_source: RestoreConfigSource,
    require_unknown_config: bool,
    expected_source_revision: Option<String>,
    check_source_health: bool,
}

impl Launcher {
    pub fn new(config: &Config) -> Result<Self> {
        let socket = Socket::from_config(config.tmux_socket().as_deref());
        let tmux = Tmux::with_timeout(socket.clone(), LAUNCH_TIMEOUT);
        let prefix = if config.naming.host_prefix {
            naming::host_prefix()
        } else {
            None
        };
        super::launch::require_tmux()?;
        let claude = super::launch::claude_bin()?;
        let taken = tmux.session_names()?;
        Ok(Launcher {
            tmux,
            prefix,
            taken,
            claude,
            quiet: false,
            provider_default: false,
            config_source: RestoreConfigSource::Recorded,
            require_unknown_config: false,
            expected_source_revision: None,
            check_source_health: false,
        })
    }

    pub fn provider_default(mut self, apply: bool) -> Self {
        self.provider_default = apply;
        self
    }

    fn config_source(mut self, source: RestoreConfigSource, require_unknown: bool) -> Self {
        self.config_source = source;
        self.check_source_health = true;
        self.require_unknown_config = require_unknown;
        self
    }

    fn expected_source_revision(mut self, revision: Option<String>) -> Self {
        self.expected_source_revision = revision;
        self
    }

    /// Print nothing on stdout; errors still go to stderr.
    pub fn quiet(mut self) -> Self {
        self.quiet = true;
        self
    }

    /// Restore `slot` (planned as [`Verdict::Restore`]) in its own tmux session.
    pub fn launch(&mut self, registry: &Registry, slot: &Slot) -> Result<Launched> {
        let label = display_name(slot);
        if self.require_unknown_config && !source_is_unknown(slot) {
            return Ok(preflight_failure(
                ResumePreflightError::ConfigSourceChanged.into(),
            ));
        }
        let mut planner = super::restore_source::Planner::for_restore();
        if let Some(expected) = self.expected_source_revision.as_deref() {
            let plan = planner.plan(slot);
            if plan.issue == Some("config_source_unavailable") {
                return Ok(preflight_failure(
                    super::restore_source::Issue::Unavailable.into(),
                ));
            }
            if plan.revision != expected {
                return Ok(preflight_failure(
                    ResumePreflightError::ConfigSourceChanged.into(),
                ));
            }
        }
        let (source_home, argv) = if self.check_source_health {
            let validated = match planner.validate_choice(slot, &self.config_source) {
                Ok(source) => source,
                Err(issue) => return Ok(preflight_failure(issue.into())),
            };
            let home = validated.home().to_string_lossy().into_owned();
            (
                Some(home),
                super::restart::resume_argv_with_validated_source(
                    &self.claude,
                    slot,
                    self.provider_default,
                    &validated,
                ),
            )
        } else {
            (
                None,
                super::restart::resume_argv(&self.claude, slot, self.provider_default),
            )
        };
        let mut owned_argv = match argv {
            Ok(argv) => argv,
            Err(error) => return Ok(preflight_failure(error)),
        };
        let source = match &self.config_source {
            RestoreConfigSource::Recorded => super::working_directory::source(slot),
            RestoreConfigSource::Default => moss_core::provider::LaunchSource::Unset,
            RestoreConfigSource::Directory(path) => {
                moss_core::provider::LaunchSource::Explicit(path.clone())
            }
        };
        let working =
            match super::working_directory::context_for_source(&source).and_then(|context| {
                Ok(moss_core::working_directory::check(
                    Path::new(&slot.cwd),
                    &context,
                )?)
            }) {
                Ok(working) => working,
                Err(error) => return Ok(preflight_failure(error)),
            };
        if let Err(error) = moss_core::working_directory::recheck(&working) {
            return Ok(preflight_failure(error.into()));
        }
        let home = match launcher_home(source_home.as_deref().map(Path::new), &working) {
            Ok(home) => home,
            Err(error) => return Ok(preflight_failure(error.into())),
        };
        let cwd = working.canonical().to_string_lossy().into_owned();
        // 所有Launcher入口都固定同一cwd能力的HOME；adopt不扩来源健康检查。
        let home_index = if owned_argv.get(1).is_some_and(|arg| arg == "-u") {
            3
        } else {
            1
        };
        owned_argv.insert(home_index, format!("HOME={}", home.display()));
        // Claim with the time of launch, not of planning: a prompt may have waited.
        match claim_with_source_guard(registry, slot, self.require_unknown_config)? {
            SourceClaim::Claimed => {}
            SourceClaim::Changed if self.expected_source_revision.is_some() => {
                return Ok(preflight_failure(
                    ResumePreflightError::ConfigSourceChanged.into(),
                ));
            }
            SourceClaim::Changed => return Ok(Launched::Changed),
            SourceClaim::SourceChanged => {
                return Ok(preflight_failure(
                    ResumePreflightError::ConfigSourceChanged.into(),
                ))
            }
        }
        let tmux_name = naming::unique(
            &naming::session_name(self.prefix.as_deref(), &label),
            &self.taken,
            None,
        );
        if let Err(error) = moss_core::working_directory::recheck(&working) {
            registry.clear_restore_started(&slot.slot_id)?;
            return Ok(preflight_failure(error.into()));
        }
        let argv: Vec<&str> = owned_argv.iter().map(String::as_str).collect();
        if let Err(e) = moss_core::tmux::ensure_access(&self.tmux, &cwd) {
            let message = tr!(
                format!("could not restore {label}: {e:#}"),
                format!("没能恢复 {label}：{e:#}")
            );
            eprintln!("cc-moss: {message}");
            registry.clear_restore_started(&slot.slot_id)?;
            return Ok(Launched::Failed(Reason::TmuxFailed, message));
        }
        let mut failure: Option<(Reason, String)> = None;
        // A timeout or error does not prove the session was not created:
        // look for it before deciding.
        let pane = match self.tmux.new_session(&tmux_name, &cwd, &argv) {
            Ok(pane) => Some(pane),
            // Only a pane that runs this very resume counts: a same-named
            // session someone else created must not be adopted.
            Err(e) => self
                .tmux
                .pane_of(&tmux_name)
                .filter(|p| {
                    p.start_command
                        .contains(&format!("--resume {}", slot.session_id))
                })
                .or_else(|| {
                    let message = tr!(
                        format!("could not restore {label}: {e:#}"),
                        format!("没能恢复 {label}：{e:#}")
                    );
                    eprintln!("cc-moss: {message}");
                    failure = Some((Reason::TmuxFailed, message));
                    None
                }),
        };
        let pane = pane.filter(|pane| {
            let dead = super::launch::exited_at_start(&self.tmux, pane);
            if dead {
                let message = super::launch::exited_message(&label, &self.claude, &slot.cwd);
                eprintln!("cc-moss: {message}");
                failure = Some((Reason::ExitedAtOnce, message));
            }
            !dead
        });
        let Some(pane) = pane else {
            if let Err(e) = registry.clear_restore_started(&slot.slot_id) {
                eprintln!("cc-moss: {e:#}");
            }
            let (reason, message) = failure.unwrap_or_else(|| {
                (
                    Reason::TmuxFailed,
                    tr!(
                        format!("could not restore {label}"),
                        format!("没能恢复 {label}")
                    ),
                )
            });
            return Ok(Launched::Failed(reason, message));
        };
        let _ = self.tmux.set_display_name(&pane.session_id, &label);
        let loc = TmuxLoc {
            socket: socket_path(&self.tmux),
            session_id: Some(pane.session_id.clone()),
            session_name: Some(tmux_name.clone()),
            pane_id: pane.pane_id.clone(),
        };
        registry.set_restored_process(
            &slot.slot_id,
            pane.pid,
            proc::start_time(pane.pid).as_deref(),
            &loc,
            now_ms(),
        )?;
        registry.close_superseded(&slot.slot_id, &slot.session_id, now_ms())?;
        let confirmed = super::restart::wait_for_start(registry, slot)?;
        if !self.quiet {
            println!(
                "{}",
                tr!(
                    format!(
                        "{} {label} in tmux session {tmux_name}{}",
                        if confirmed { "Started" } else { "Starting" },
                        if confirmed {
                            "."
                        } else {
                            "; waiting for the session start event."
                        }
                    ),
                    format!(
                        "{} {label}（tmux 会话 {tmux_name}）{}",
                        if confirmed {
                            "已启动"
                        } else {
                            "正在启动"
                        },
                        if confirmed {
                            "。"
                        } else {
                            "，等待会话启动事件。"
                        }
                    )
                )
            );
        }
        self.taken.push(tmux_name.clone());
        Ok(Launched::Started(tmux_name))
    }
}

/// Launching may start the tmux server and is something the user waits on.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(5);

/// The server's socket path, recorded like the hooks do (from `$TMUX`).
fn socket_path(tmux: &Tmux<ProcessRunner>) -> String {
    tmux.socket_path().unwrap_or_default()
}

pub fn display_name(slot: &Slot) -> String {
    slot.name
        .clone()
        .unwrap_or_else(|| slot.session_id.chars().take(8).collect())
}

fn nothing_to_restore() -> &'static str {
    tr!(
        "Nothing to restore. Everything made it through.",
        "没有需要恢复的会话，全员健在。"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::event::{Context, Event, StartSource};
    use moss_core::provider::{LaunchSource, Snapshot};

    #[test]
    fn config_source_guard_detects_two_connection_change_without_claim() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.db");
        let mut registry = Registry::open(&path).unwrap();
        registry
            .apply(
                &Event::Start {
                    session_id: "source-race".into(),
                    cwd: "/work".into(),
                    title: None,
                    transcript_path: None,
                    source: StartSource::Startup,
                },
                &Context {
                    pid: None,
                    pid_start: None,
                    tmux: None,
                    launch_args: None,
                    now_ms: 1000,
                },
            )
            .unwrap();
        let planned = registry.slots().unwrap().remove(0);
        assert!(source_is_unknown(&planned));
        let other = Registry::open(&path).unwrap();
        let known = Snapshot {
            launch_source: LaunchSource::Unset,
            ..Snapshot::default()
        }
        .to_json();
        other.set_provider(&planned.slot_id, &known, "[]").unwrap();
        assert_eq!(
            registry.slots().unwrap()[0].last_active_at_ms,
            planned.last_active_at_ms
        );
        assert_eq!(
            claim_with_source_guard(&registry, &planned, true).unwrap(),
            SourceClaim::SourceChanged
        );
        assert!(registry.slots().unwrap()[0].restore_started_at_ms.is_none());
        assert!(matches!(
            preflight_failure(ResumePreflightError::ConfigSourceChanged.into()),
            Launched::Failed(Reason::ConfigSourceChanged, _)
        ));
        assert_eq!(
            claim_with_source_guard(&registry, &planned, false).unwrap(),
            SourceClaim::Changed
        );
        let current = registry.slots().unwrap().remove(0);
        other.set_provider(&current.slot_id, &known, "[]").unwrap();
        assert_eq!(
            claim_with_source_guard(&registry, &current, false).unwrap(),
            SourceClaim::Claimed
        );
    }

    #[test]
    fn config_source_guard_accepts_unchanged_null_snapshot() {
        let mut registry = Registry::open_in_memory().unwrap();
        registry
            .apply(
                &Event::Start {
                    session_id: "source-null".into(),
                    cwd: "/work".into(),
                    title: None,
                    transcript_path: None,
                    source: StartSource::Startup,
                },
                &Context {
                    pid: None,
                    pid_start: None,
                    tmux: None,
                    launch_args: None,
                    now_ms: 1000,
                },
            )
            .unwrap();
        let planned = registry.slots().unwrap().remove(0);
        assert!(planned.provider_started.is_none());
        assert_eq!(
            claim_with_source_guard(&registry, &planned, true).unwrap(),
            SourceClaim::Claimed
        );
    }
}
