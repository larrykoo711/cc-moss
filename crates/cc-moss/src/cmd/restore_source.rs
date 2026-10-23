//! 只读恢复来源计划；不读取凭据、不保存配置正文，不推测旧来源。

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

const JSON_LIMIT: u64 = 1024 * 1024;
const LIST_BUDGET: Duration = Duration::from_millis(200);
const RESTORE_BUDGET: Duration = Duration::from_secs(2);
const MAX_PROBES: usize = 64;
const MAX_WORKERS: usize = 2;
static ACTIVE_WORKERS: OnceLock<Arc<AtomicUsize>> = OnceLock::new();

struct WorkerPermit(Arc<AtomicUsize>);
impl WorkerPermit {
    fn acquire(active: &Arc<AtomicUsize>) -> Option<Self> {
        active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_WORKERS).then_some(n + 1)
            })
            .ok()?;
        Some(Self(active.clone()))
    }
}
impl Drop for WorkerPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

use moss_core::provider::{LaunchSource, Snapshot};
use moss_core::slot::Slot;
use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Issue {
    Unknown,
    Unsupported,
    InvalidPath,
    MissingDirectory,
    NotDirectory,
    Unreadable,
    InvalidSettings,
    Unavailable,
}

impl Issue {
    pub fn code(self) -> &'static str {
        match self {
            Self::Unknown => "config_source_unknown",
            Self::Unsupported => "config_source_unsupported",
            Self::InvalidPath => "invalid_config_source",
            Self::MissingDirectory => "config_directory_missing",
            Self::NotDirectory => "config_not_directory",
            Self::Unreadable => "config_source_unreadable",
            Self::InvalidSettings => "config_settings_invalid",
            Self::Unavailable => "config_source_unavailable",
        }
    }
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unknown => "the session's config source is unknown; explicitly choose a source for this restore",
            Self::Unsupported => "the session used config arguments that cannot be replayed safely",
            Self::InvalidPath => "the config directory must be an absolute path without control characters",
            Self::MissingDirectory => "the recorded config directory no longer exists; choose a configuration location",
            Self::NotDirectory => "the config location is not a directory; choose a configuration directory",
            Self::Unreadable => "the config source or target home cannot be accessed; check permissions or choose another location",
            Self::InvalidSettings => "settings.json is not a valid JSON object; repair it or choose another configuration location",
            Self::Unavailable => "configuration checking could not finish within its time or size budget; try again after checking the location",
        })
    }
}
impl std::error::Error for Issue {}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub kind: &'static str,
    pub status: &'static str,
    pub display_path: Option<String>,
    pub default_path: Option<String>,
    pub revision: String,
    pub issue: Option<&'static str>,
}

/// 只能由有界健康检查签发，参数builder不再做同步来源FS读取。
pub struct ValidatedConfigSource {
    choice: super::restart::RestoreConfigSource,
    source: LaunchSource,
    slot_id: String,
    session_id: String,
    provider_started: Option<String>,
    home: PathBuf,
}
impl ValidatedConfigSource {
    pub fn choice(&self) -> &super::restart::RestoreConfigSource {
        &self.choice
    }
    pub fn home(&self) -> &Path {
        &self.home
    }
    pub fn matches(&self, slot: &Slot) -> bool {
        let current = match &self.choice {
            super::restart::RestoreConfigSource::Recorded => {
                slot.provider_started
                    .as_deref()
                    .and_then(Snapshot::from_json)
                    .unwrap_or_default()
                    .launch_source
            }
            super::restart::RestoreConfigSource::Default => LaunchSource::Unset,
            super::restart::RestoreConfigSource::Directory(path) => {
                LaunchSource::Explicit(path.clone())
            }
        };
        self.slot_id == slot.slot_id
            && self.session_id == slot.session_id
            && self.provider_started == slot.provider_started
            && self.source == current
    }
}

#[derive(Clone)]
struct Health {
    issue: Option<Issue>,
    metadata: String,
    home: Option<PathBuf>,
}

impl Health {
    fn unavailable() -> Self {
        Self {
            issue: Some(Issue::Unavailable),
            metadata: "unavailable".into(),
            home: None,
        }
    }
}

type Probe = Box<dyn FnMut(&LaunchSource) -> Health + Send>;

pub struct Planner {
    home: Option<PathBuf>,
    cache: HashMap<String, Health>,
    worker: Option<(SyncSender<LaunchSource>, Receiver<Health>)>,
    probe: Option<Probe>,
    active_workers: Arc<AtomicUsize>,
    deadline: Instant,
    exhausted: bool,
    probes: usize,
    verified_home: Option<PathBuf>,
}

impl Planner {
    pub fn current() -> Self {
        Self::with_budget(target_home(), LIST_BUDGET)
    }

    pub fn for_restore() -> Self {
        Self::with_budget(target_home(), RESTORE_BUDGET)
    }

    #[cfg(test)]
    fn new(home: Option<PathBuf>) -> Self {
        let mut planner = Self::with_budget(home, RESTORE_BUDGET);
        planner.active_workers = Arc::new(AtomicUsize::new(0));
        planner
    }

    fn with_budget(home: Option<PathBuf>, budget: Duration) -> Self {
        let worker_home = home.clone();
        Self::with_probe(home, budget, move |source| {
            probe(worker_home.as_deref(), source)
        })
    }

    fn with_probe<F>(home: Option<PathBuf>, budget: Duration, probe: F) -> Self
    where
        F: FnMut(&LaunchSource) -> Health + Send + 'static,
    {
        Self {
            home,
            cache: HashMap::new(),
            worker: None,
            probe: Some(Box::new(probe)),
            active_workers: ACTIVE_WORKERS
                .get_or_init(|| Arc::new(AtomicUsize::new(0)))
                .clone(),
            deadline: Instant::now() + budget,
            exhausted: false,
            probes: 0,
            verified_home: None,
        }
    }

    fn start_worker(&mut self) -> bool {
        if self.worker.is_some() {
            return true;
        }
        let Some(permit) = WorkerPermit::acquire(&self.active_workers) else {
            return false;
        };
        let Some(mut probe) = self.probe.take() else {
            return false;
        };
        let (requests, tasks) = mpsc::sync_channel::<LaunchSource>(1);
        let (results_tx, results) = mpsc::sync_channel(1);
        // 许可由真实worker持有到退出；超时不能释放许可并启动替代线程。
        let spawned = std::thread::Builder::new()
            .name("restore-source-probe".into())
            .spawn(move || {
                let _permit = permit;
                while let Ok(source) = tasks.recv() {
                    if results_tx.send(probe(&source)).is_err() {
                        break;
                    }
                }
            })
            .is_ok();
        if spawned {
            self.worker = Some((requests, results));
        }
        spawned
    }

    fn exhaust(&mut self) -> Health {
        self.exhausted = true;
        self.verified_home = None;
        self.cache.clear();
        self.worker = None;
        Health::unavailable()
    }

    pub fn home(&mut self) -> Result<&Path, Issue> {
        if self.verified_home.is_none() {
            self.health(&LaunchSource::Unknown);
        }
        if self.exhausted {
            return Err(Issue::Unavailable);
        }
        self.verified_home.as_deref().ok_or(Issue::Unreadable)
    }

    fn default_path(&mut self) -> Option<String> {
        self.home()
            .ok()
            .map(|h| h.join(".claude").to_string_lossy().into_owned())
    }

    pub fn plan(&mut self, slot: &Slot) -> Plan {
        let snapshot = slot
            .provider_started
            .as_deref()
            .and_then(Snapshot::from_json)
            .unwrap_or_default();
        let source = &snapshot.launch_source;
        let health = self.health(source);
        let (kind, path) = match source {
            LaunchSource::Unknown => ("unknown", None),
            LaunchSource::Unset => ("unset", self.default_path()),
            LaunchSource::Explicit(path) => {
                ("explicit", safe_path(Path::new(path)).then(|| path.clone()))
            }
        };
        let default_path = self.default_path();
        let issue = if !snapshot.unreplayable_args.is_empty() {
            Some(Issue::Unsupported)
        } else {
            health.issue
        };
        // 仅安全来源/标记及文件元数据；不hash配置内容、供应商值或凭据。
        let material = serde_json::to_vec(&(
            &slot.slot_id,
            &slot.session_id,
            source,
            &snapshot.unreplayable_args,
            self.home.as_ref(),
            &health.metadata,
            issue.map(Issue::code),
        ))
        .expect("source revision is serializable");
        let mut hash = 0xcbf29ce484222325u64;
        for byte in material {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        Plan {
            kind,
            status: match issue {
                None => "ready",
                Some(Issue::Unsupported | Issue::Unavailable) => "blocked",
                Some(_) => "needs_location",
            },
            display_path: if issue == Some(Issue::Unavailable) {
                None
            } else {
                path
            },
            default_path: if issue == Some(Issue::Unavailable) {
                None
            } else {
                default_path
            },
            revision: if issue == Some(Issue::Unavailable) {
                String::new()
            } else {
                format!("{hash:016x}")
            },
            issue: issue.map(Issue::code),
        }
    }

    pub fn validate_choice(
        &mut self,
        slot: &Slot,
        choice: &super::restart::RestoreConfigSource,
    ) -> Result<ValidatedConfigSource, Issue> {
        let snapshot = slot
            .provider_started
            .as_deref()
            .and_then(Snapshot::from_json)
            .unwrap_or_default();
        if !snapshot.unreplayable_args.is_empty() {
            return Err(Issue::Unsupported);
        }
        let source = match choice {
            super::restart::RestoreConfigSource::Recorded => snapshot.launch_source,
            super::restart::RestoreConfigSource::Default => LaunchSource::Unset,
            super::restart::RestoreConfigSource::Directory(path) => {
                LaunchSource::Explicit(path.clone())
            }
        };
        self.validate(&source)?;
        Ok(ValidatedConfigSource {
            choice: choice.clone(),
            source,
            slot_id: slot.slot_id.clone(),
            session_id: slot.session_id.clone(),
            provider_started: slot.provider_started.clone(),
            home: self.home()?.to_path_buf(),
        })
    }

    pub fn validate(&mut self, source: &LaunchSource) -> Result<(), Issue> {
        match self.health(source).issue {
            Some(issue) => Err(issue),
            None => Ok(()),
        }
    }

    fn health(&mut self, source: &LaunchSource) -> Health {
        if self.exhausted {
            return Health::unavailable();
        }
        let key =
            serde_json::to_string(&(source, &self.home)).expect("source cache key is serializable");
        if let Some(health) = self.cache.get(&key) {
            return health.clone();
        }
        if self.probes >= MAX_PROBES || Instant::now() >= self.deadline || !self.start_worker() {
            return self.exhaust();
        }
        self.probes += 1;
        let (requests, results) = self.worker.as_ref().expect("worker started");
        if requests.try_send(source.clone()).is_err() {
            return self.exhaust();
        }
        let health =
            match results.recv_timeout(self.deadline.saturating_duration_since(Instant::now())) {
                Ok(health) => health,
                Err(_) => return self.exhaust(),
            };
        self.verified_home = health.home.clone();
        self.cache.insert(key, health.clone());
        health
    }
}

fn target_home() -> Option<PathBuf> {
    if std::env::var_os(moss_core::moss::ROLE_ENV).is_some() {
        return None;
    }
    std::env::var_os("HOME").map(PathBuf::from)
}

// 所有可能阻塞的FS操作只在唯一worker中执行。
fn probe(home: Option<&Path>, source: &LaunchSource) -> Health {
    let home = home.and_then(|path| {
        directory(path, false).ok()?;
        path.canonicalize().ok().filter(|path| safe_path(path))
    });
    let mut metadata = home.as_deref().map(stamp).unwrap_or_default();
    let (path, allow_missing) = match source {
        LaunchSource::Unknown => {
            return Health {
                issue: Some(Issue::Unknown),
                metadata,
                home,
            }
        }
        LaunchSource::Unset => (home.as_ref().map(|h| h.join(".claude")), true),
        LaunchSource::Explicit(path) => (Some(PathBuf::from(path)), false),
    };
    let issue = if home.is_none() {
        Some(Issue::Unreadable)
    } else if let Some(path) = &path {
        metadata.push_str(&stamp(path));
        metadata.push_str(&stamp(&path.join("settings.json")));
        directory(path, allow_missing)
            .and_then(|()| settings(&path.join("settings.json")))
            .err()
    } else {
        Some(Issue::Unreadable)
    };
    Health {
        issue,
        metadata,
        home,
    }
}

fn safe_path(path: &Path) -> bool {
    path.is_absolute()
        && path
            .to_str()
            .is_some_and(|p| !p.chars().any(char::is_control))
}

fn directory(path: &Path, allow_missing: bool) -> Result<(), Issue> {
    if !safe_path(path) {
        return Err(Issue::InvalidPath);
    }
    match fs::metadata(path) {
        Ok(meta) if !meta.is_dir() => Err(Issue::NotDirectory),
        Ok(_) => fs::read_dir(path)
            .map(|_| ())
            .map_err(|_| Issue::Unreadable),
        Err(e) if e.kind() == io::ErrorKind::NotFound && allow_missing => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Err(Issue::MissingDirectory),
        Err(_) => Err(Issue::Unreadable),
    }
}

// serde只遍历键值，不构造Value/String，也不返回含正文片段的解析诊断。
struct JsonObject;
impl<'de> Deserialize<'de> for JsonObject {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;
        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = JsonObject;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<JsonObject, A::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(JsonObject)
            }
        }
        deserializer.deserialize_map(ObjectVisitor)
    }
}

fn settings(path: &Path) -> Result<(), Issue> {
    use std::os::unix::fs::OpenOptionsExt;
    // 防止伪装为settings.json的FIFO阻塞列表刷新。
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(Issue::Unreadable),
    };
    let metadata = file.metadata().map_err(|_| Issue::Unreadable)?;
    if !metadata.is_file() {
        return Err(Issue::Unreadable);
    }
    if metadata.len() > JSON_LIMIT {
        return Err(Issue::Unavailable);
    }
    limited_json(file)
}

fn limited_json(reader: impl Read) -> Result<(), Issue> {
    let mut limited = reader.take(JSON_LIMIT + 1);
    let result = {
        let mut parser = serde_json::Deserializer::from_reader(BufReader::new(&mut limited));
        JsonObject::deserialize(&mut parser)
            .and_then(|_| parser.end())
            .map_err(|e| {
                if e.is_io() {
                    Issue::Unreadable
                } else {
                    Issue::InvalidSettings
                }
            })
    };
    if JSON_LIMIT + 1 - limited.limit() > JSON_LIMIT {
        return Err(Issue::Unavailable);
    }
    result
}

fn stamp(path: &Path) -> String {
    use std::os::unix::fs::MetadataExt;
    match fs::metadata(path) {
        Ok(m) => format!(
            "{}:{}:{}:{}:{}:{}:{}:{};",
            m.dev(),
            m.ino(),
            m.mode(),
            m.len(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec()
        ),
        Err(e) => format!("{:?};", e.kind()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_slot() -> Slot {
        use moss_core::event::{Context, Event, StartSource};
        let mut registry = moss_core::registry::Registry::open_in_memory().unwrap();
        registry
            .apply(
                &Event::Start {
                    session_id: "source-builder".into(),
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
        registry.slots().unwrap().remove(0)
    }

    #[test]
    fn restore_source_plan_home_alias_matches_cwd_capability_and_retargeting_refuses() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first-home");
        let second = temp.path().join("second-home");
        let work = temp.path().join("work");
        let alias = temp.path().join("home-link");
        for path in [&first, &second, &work] {
            fs::create_dir(path).unwrap();
        }
        std::os::unix::fs::symlink(&first, &alias).unwrap();
        let mut planner = Planner::new(Some(alias.clone()));
        let source = planner
            .validate_choice(
                &test_slot(),
                &super::super::restart::RestoreConfigSource::Default,
            )
            .unwrap();
        let context = moss_core::working_directory::Context::new(alias.clone(), vec![]).unwrap();
        let working = moss_core::working_directory::check(&work, &context).unwrap();
        assert_eq!(
            super::super::restore::launcher_home(Some(source.home()), &working).unwrap(),
            first.canonicalize().unwrap()
        );
        fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&second, &alias).unwrap();
        let changed = moss_core::working_directory::check(&work, &context).unwrap();
        assert_eq!(
            super::super::restore::launcher_home(Some(source.home()), &changed),
            Err(moss_core::working_directory::Error::Changed)
        );
    }

    #[test]
    fn restore_source_plan_builder_requires_validated_directory_and_performs_no_source_stat() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("config");
        let choice = super::super::restart::RestoreConfigSource::Directory(
            dir.to_string_lossy().into_owned(),
        );
        let slot = test_slot();
        let mut missing = Planner::new(Some(home.path().to_path_buf()));
        assert!(matches!(
            missing.validate_choice(&slot, &choice),
            Err(Issue::MissingDirectory)
        ));
        fs::create_dir(&dir).unwrap();
        let mut planner = Planner::new(Some(home.path().to_path_buf()));
        let validated = planner.validate_choice(&slot, &choice).unwrap();
        fs::remove_dir(&dir).unwrap();
        // 预检后变化属于已声明的FS竞态；builder不得回主线程做第二次stat。
        let argv = super::super::restart::resume_argv_with_validated_source(
            "claude", &slot, false, &validated,
        )
        .unwrap();
        assert!(argv.contains(&format!("CLAUDE_CONFIG_DIR={}", dir.display())));
        assert_eq!(validated.home(), home.path().canonicalize().unwrap());
        let mut other = slot.clone();
        other.session_id = "different-session".into();
        assert!(super::super::restart::resume_argv_with_validated_source(
            "claude", &other, false, &validated
        )
        .is_err());
        other = slot.clone();
        other.provider_started = Some(
            Snapshot {
                launch_source: LaunchSource::Unset,
                ..Default::default()
            }
            .to_json(),
        );
        assert!(super::super::restart::resume_argv_with_validated_source(
            "claude", &other, false, &validated
        )
        .is_err());
    }

    #[test]
    fn restore_source_plan_budget_shared_deadline_does_not_join_or_rebuild_workers() {
        let active = Arc::new(AtomicUsize::new(0));
        let (release, blocked) = mpsc::channel();
        let mut planner = Planner::with_probe(
            Some(PathBuf::from("/home")),
            Duration::from_millis(20),
            move |_| {
                blocked.recv().unwrap();
                Health {
                    issue: None,
                    metadata: "old-ready".into(),
                    home: Some(PathBuf::from("/home")),
                }
            },
        );
        planner.active_workers = active.clone();
        assert_eq!(active.load(Ordering::Acquire), 0, "worker is lazy");
        let start = Instant::now();
        assert_eq!(
            planner.validate(&LaunchSource::Unset),
            Err(Issue::Unavailable)
        );
        for n in 0..100 {
            assert_eq!(
                planner.validate(&LaunchSource::Explicit(format!("/source/{n}"))),
                Err(Issue::Unavailable)
            );
        }
        assert_eq!(planner.probes, 1);
        assert!(planner.default_path().is_none());
        assert_eq!(planner.home().unwrap_err(), Issue::Unavailable);
        drop(planner);
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "drop must not join blocked probe"
        );
        assert_eq!(
            active.load(Ordering::Acquire),
            1,
            "timeout does not release worker permit"
        );
        release.send(()).unwrap();
        let end = Instant::now() + Duration::from_secs(1);
        while active.load(Ordering::Acquire) != 0 && Instant::now() < end {
            std::thread::yield_now();
        }
        assert_eq!(active.load(Ordering::Acquire), 0);
    }

    #[test]
    fn restore_source_plan_budget_caps_workers_across_planners_until_actual_exit() {
        let active = Arc::new(AtomicUsize::new(0));
        let mut releases = Vec::new();
        for _ in 0..MAX_WORKERS {
            let (release, blocked) = mpsc::channel();
            let mut planner = Planner::with_probe(None, Duration::from_millis(20), move |_| {
                blocked.recv().unwrap();
                Health::unavailable()
            });
            planner.active_workers = active.clone();
            assert_eq!(
                planner.validate(&LaunchSource::Unset),
                Err(Issue::Unavailable)
            );
            drop(planner);
            releases.push(release);
        }
        assert_eq!(active.load(Ordering::Acquire), MAX_WORKERS);
        for _ in 0..20 {
            let mut rejected = Planner::with_probe(None, Duration::from_secs(2), |_| {
                panic!("no permit means no probe")
            });
            rejected.active_workers = active.clone();
            assert_eq!(
                rejected.validate(&LaunchSource::Unset),
                Err(Issue::Unavailable)
            );
            assert!(rejected.worker.is_none());
        }
        for release in releases {
            release.send(()).unwrap();
        }
        let end = Instant::now() + Duration::from_secs(1);
        while active.load(Ordering::Acquire) != 0 && Instant::now() < end {
            std::thread::yield_now();
        }
        assert_eq!(active.load(Ordering::Acquire), 0);
    }

    #[test]
    fn restore_source_plan_budget_counts_failed_probes_and_has_a_hard_limit() {
        let mut planner = Planner::with_probe(None, Duration::from_secs(2), |_| Health {
            issue: Some(Issue::MissingDirectory),
            metadata: String::new(),
            home: None,
        });
        planner.active_workers = Arc::new(AtomicUsize::new(0));
        for n in 0..MAX_PROBES {
            assert_eq!(
                planner.validate(&LaunchSource::Explicit(format!("/missing/{n}"))),
                Err(Issue::MissingDirectory)
            );
        }
        assert_eq!(
            planner.validate(&LaunchSource::Unset),
            Err(Issue::Unavailable)
        );
        assert_eq!(planner.probes, MAX_PROBES);
    }

    #[test]
    fn restore_source_plan_budget_limit_plus_one_detects_growth_without_metadata() {
        let large = format!("{{\"large\":\"{}\"}}", "x".repeat(JSON_LIMIT as usize));
        assert_eq!(limited_json(large.as_bytes()), Err(Issue::Unavailable));
        let exact = format!("{{}}{}", " ".repeat(JSON_LIMIT as usize - 2));
        assert!(limited_json(exact.as_bytes()).is_ok());
        assert_eq!(
            limited_json(format!("{exact} ").as_bytes()),
            Err(Issue::Unavailable)
        );
    }

    #[test]
    fn restore_source_plan_oversized_settings_are_not_ready() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("settings.json");
        fs::write(
            &path,
            format!("{{\"large\":\"{}\"}}", "x".repeat(1024 * 1024)),
        )
        .unwrap();
        assert!(settings(&path).is_err());
    }

    #[test]
    fn restore_source_plan_missing_optional_settings_and_default_directory_are_valid() {
        let home = tempfile::tempdir().unwrap();
        let mut planner = Planner::new(Some(home.path().to_path_buf()));
        assert!(planner.validate(&LaunchSource::Unset).is_ok());
        assert!(planner
            .validate(&LaunchSource::Explicit(
                home.path().to_string_lossy().into_owned()
            ))
            .is_ok());
    }

    #[test]
    fn restore_source_plan_json_validation_ignores_provider_field_shapes_and_secrets() {
        let home = tempfile::tempdir().unwrap();
        let settings_path = home.path().join("settings.json");
        fs::write(
            &settings_path,
            r#"{"env":42,"model":[],"secret":"never-return-this-value"}"#,
        )
        .unwrap();
        assert!(settings(&settings_path).is_ok());
        for value in [
            "[]",
            "null",
            "42",
            "{} {}",
            "{\"secret\":\"never-return-this-value\"",
        ] {
            fs::write(&settings_path, value).unwrap();
            let error = settings(&settings_path).unwrap_err();
            assert_eq!(error, Issue::InvalidSettings);
            assert!(!error.to_string().contains("never-return-this-value"));
        }
    }

    #[test]
    fn restore_source_plan_directory_health_distinguishes_missing_file_and_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        assert_eq!(
            directory(&home.path().join("missing"), false),
            Err(Issue::MissingDirectory)
        );
        let file = home.path().join("file");
        fs::write(&file, "{}").unwrap();
        assert_eq!(directory(&file, false), Err(Issue::NotDirectory));
        assert_eq!(
            directory(Path::new("relative"), false),
            Err(Issue::InvalidPath)
        );
        // root可读取mode=000，不能把权限位当实际可访问性的证据。
        if unsafe { libc::geteuid() } != 0 {
            fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
            assert_eq!(settings(&file), Err(Issue::Unreadable));
            fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
            let dir = home.path().join("locked");
            fs::create_dir(&dir).unwrap();
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o000)).unwrap();
            let result = directory(&dir, false);
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(result, Err(Issue::Unreadable));
        }
    }

    #[test]
    fn restore_source_plan_cache_is_per_source_and_per_invocation() {
        let home = tempfile::tempdir().unwrap();
        let source = LaunchSource::Explicit(home.path().to_string_lossy().into_owned());
        let mut first = Planner::new(Some(home.path().to_path_buf()));
        assert!(first.validate(&source).is_ok());
        fs::write(home.path().join("settings.json"), "[]").unwrap();
        assert!(first.validate(&source).is_ok());
        assert_eq!(first.cache.len(), 1);
        let mut execution = Planner::new(Some(home.path().to_path_buf()));
        assert_eq!(execution.validate(&source), Err(Issue::InvalidSettings));
    }

    #[test]
    fn restore_source_plan_unreliable_home_has_no_default_proposal() {
        let mut planner = Planner::new(Some(PathBuf::from("relative")));
        assert!(planner.default_path().is_none());
        assert_eq!(
            planner.validate(&LaunchSource::Unset),
            Err(Issue::Unreadable)
        );
    }
}
