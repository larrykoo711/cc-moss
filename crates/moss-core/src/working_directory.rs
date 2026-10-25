//! 会话工作目录政策；配置来源可读与配置目录不可作为cwd是两回事。
use std::ffi::{CStr, CString};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use std::{fs, io};

const TIMEOUT: Duration = Duration::from_secs(2);
const MAX_WORKERS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Forbidden,
    Missing,
    NotDirectory,
    Unreadable,
    Unavailable,
    Invalid,
    Changed,
}
impl Error {
    pub fn code(self) -> &'static str {
        match self {
            Self::Forbidden => "cwd_forbidden",
            Self::Missing => "cwd_missing",
            Self::NotDirectory => "cwd_not_directory",
            Self::Unreadable => "cwd_unreadable",
            Self::Unavailable => "cwd_unavailable",
            Self::Invalid => "cwd_invalid",
            Self::Changed => "cwd_changed",
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
        Self::Forbidden => "this system, home or configuration directory cannot be used as a session working directory; choose a project folder",
        Self::Missing => "the working directory no longer exists",
        Self::NotDirectory => "the working location is not a directory",
        Self::Unreadable => "the working directory cannot be accessed",
        Self::Unavailable => "the working directory could not be checked within the safety budget",
        Self::Invalid => "the working directory and launch context must be absolute paths without control characters",
        Self::Changed => "the working directory or launch context changed; start again",
    })
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Context {
    target_home: PathBuf,
    config_roots: Vec<PathBuf>,
    config_file: Option<PathBuf>,
    source_identity: Option<String>,
}
impl Context {
    pub fn new(target_home: PathBuf, config_roots: Vec<PathBuf>) -> Result<Self, Error> {
        if !safe(&target_home) || config_roots.iter().any(|p| !safe(p)) {
            return Err(Error::Invalid);
        }
        Ok(Self {
            target_home,
            config_roots,
            config_file: None,
            source_identity: None,
        })
    }
    pub fn with_config_file(mut self, path: PathBuf) -> Result<Self, Error> {
        if !safe(&path) {
            return Err(Error::Invalid);
        }
        self.config_file = Some(path);
        Ok(self)
    }
    pub fn with_source_identity(mut self, source: &crate::provider::LaunchSource) -> Self {
        self.source_identity =
            Some(serde_json::to_string(source).expect("source identity serializes"));
        self
    }
    pub fn target_home(&self) -> &Path {
        &self.target_home
    }
}

#[derive(Debug, Clone)]
pub struct ValidatedWorkingDirectory {
    original: PathBuf,
    canonical: PathBuf,
    context: Context,
    os_home: PathBuf,
    target_home: PathBuf,
    device: u64,
    inode: u64,
}
impl ValidatedWorkingDirectory {
    pub fn canonical(&self) -> &Path {
        &self.canonical
    }
    pub fn target_home(&self) -> &Path {
        &self.target_home
    }
}

struct Permit(Arc<AtomicUsize>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
fn bounded<T: Send + 'static>(
    budget: Duration,
    run: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    bounded_in(permits(), budget, run)
}
/// One pool for the whole process, so probes stuck on a hung filesystem cannot pile up.
#[cfg(not(any(test, feature = "test-permits")))]
fn permits() -> Arc<AtomicUsize> {
    static ACTIVE: std::sync::OnceLock<Arc<AtomicUsize>> = std::sync::OnceLock::new();
    ACTIVE.get_or_init(|| Arc::new(AtomicUsize::new(0))).clone()
}
/// Test builds count per calling thread: tests run in parallel threads of one process and
/// would otherwise take each other's permits. Only dev-dependencies enable `test-permits`.
#[cfg(any(test, feature = "test-permits"))]
fn permits() -> Arc<AtomicUsize> {
    thread_local!(static ACTIVE: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0)));
    ACTIVE.with(Arc::clone)
}
fn bounded_in<T: Send + 'static>(
    active: Arc<AtomicUsize>,
    budget: Duration,
    run: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    if budget.is_zero() {
        return Err(Error::Unavailable);
    }
    let deadline = std::time::Instant::now() + budget;
    // A compare-exchange loop rather than `fetch_update`, which newer Rust deprecates.
    let mut n = active.load(Ordering::Acquire);
    loop {
        if n >= MAX_WORKERS {
            return Err(Error::Unavailable);
        }
        match active.compare_exchange_weak(n, n + 1, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => break,
            Err(current) => n = current,
        }
    }
    let permit = Permit(active);
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("working-directory".into())
        .spawn(move || {
            // Held while the probe runs (a hung filesystem keeps it), returned before the
            // answer: the caller's next probe must not find this finished worker counted.
            let result = run();
            drop(permit);
            let _ = tx.send(result);
        })
        .map_err(|_| Error::Unavailable)?;
    rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
        .map_err(|_| Error::Unavailable)?
}

/// 恢复规划仅探测目录状态，不签发启动能力；调用方保留所有typed失败。
pub fn probe_directory(path: &Path, budget: Duration) -> Result<(), Error> {
    let path = path.to_path_buf();
    bounded(budget, move || {
        if !safe(&path) {
            return Err(Error::Invalid);
        }
        accessible(&path)
    })
}

pub fn check(path: &Path, context: &Context) -> Result<ValidatedWorkingDirectory, Error> {
    check_with_budget(path, context, TIMEOUT)
}
pub fn check_with_budget(
    path: &Path,
    context: &Context,
    budget: Duration,
) -> Result<ValidatedWorkingDirectory, Error> {
    let path = path.to_path_buf();
    let context = context.clone();
    bounded(budget, move || inspect(&path, &context))
}
pub fn recheck(directory: &ValidatedWorkingDirectory) -> Result<(), Error> {
    let previous = directory.clone();
    bounded(TIMEOUT, move || {
        let current = inspect(&previous.original, &previous.context)?;
        if current.canonical != previous.canonical
            || current.device != previous.device
            || current.inode != previous.inode
            || current.os_home != previous.os_home
            || current.target_home != previous.target_home
        {
            return Err(Error::Changed);
        }
        Ok(())
    })
}

/// 仅受控runtime叶目录创建；先验证目标投影，Exact祖先不等于递归禁止。
pub fn prepare_runtime(path: &Path, context: &Context) -> Result<ValidatedWorkingDirectory, Error> {
    let path = path.to_path_buf();
    let context = context.clone();
    let deadline = std::time::Instant::now() + TIMEOUT;
    bounded(TIMEOUT, move || {
        let policy = Policy::resolve(&context)?;
        policy.allow(&path)?;
        let target = project(&path)?;
        policy.allow(&target)?;
        if std::time::Instant::now() >= deadline {
            return Err(Error::Unavailable);
        }
        fs::create_dir_all(&target).map_err(fs_error)?;
        inspect(&path, &context)
    })
}

fn safe(path: &Path) -> bool {
    path.is_absolute()
        && path
            .to_str()
            .is_some_and(|p| !p.chars().any(char::is_control))
}
fn fs_error(error: io::Error) -> Error {
    match error.kind() {
        io::ErrorKind::NotFound => Error::Missing,
        io::ErrorKind::NotADirectory => Error::NotDirectory,
        io::ErrorKind::PermissionDenied => Error::Unreadable,
        _ => Error::Unavailable,
    }
}
fn logical(path: &Path, macos: bool) -> PathBuf {
    let mut normalized = PathBuf::from("/");
    for part in path.components() {
        match part {
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => {
                normalized.pop();
            }
            _ => {}
        }
    }
    if macos {
        if let Ok(tail) = normalized.strip_prefix("/System/Volumes/Data") {
            normalized = Path::new("/").join(tail);
        }
        for (alias, target) in [
            ("/var", "/private/var"),
            ("/etc", "/private/etc"),
            ("/tmp", "/private/tmp"),
        ] {
            if let Ok(tail) = normalized.strip_prefix(alias) {
                return Path::new(target).join(tail);
            }
        }
    }
    normalized
}

struct Policy {
    exact: Vec<PathBuf>,
    trees: Vec<PathBuf>,
    os_home: PathBuf,
    target_home: PathBuf,
    macos: bool,
}
impl Policy {
    fn resolve(context: &Context) -> Result<Self, Error> {
        let os_home = trusted_home()?.canonicalize().map_err(fs_error)?;
        let target_home = context.target_home.canonicalize().map_err(fs_error)?;
        accessible(&target_home)?;
        let mut policy = Self::table(&os_home, &target_home, cfg!(target_os = "macos"));
        let mut roots = context.config_roots.clone();
        if let Some(file) = &context.config_file {
            // cc-moss配置文件的父目录可能是HOME或Documents等容器：只禁该目录本身。
            let parent = file.parent().ok_or(Error::Invalid)?;
            let real = project(file).map_err(|_| Error::Unavailable)?;
            let real_parent = project(parent).map_err(|_| Error::Unavailable)?;
            for dir in [parent, &real_parent, real.parent().ok_or(Error::Invalid)?] {
                policy.exact.push(logical(dir, policy.macos));
            }
            let configured = bounded_config(file)?.claude.config_dir;
            if let Some(dir) = claude_config_dir(configured.as_deref(), &target_home) {
                roots.push(dir);
            }
        }
        for root in &roots {
            policy.trees.push(logical(root, policy.macos));
            policy.trees.push(logical(&project(root)?, policy.macos));
        }
        for home in [&os_home, &target_home] {
            for name in USER_CONFIG {
                policy
                    .trees
                    .push(logical(&project(&home.join(name))?, policy.macos));
            }
        }
        Ok(policy)
    }
    fn table(os_home: &Path, target_home: &Path, macos: bool) -> Self {
        let mut exact: Vec<PathBuf> = ["/", "/usr", "/opt", "/var", "/private", "/tmp", "/Volumes"]
            .iter()
            .map(|p| logical(Path::new(p), macos))
            .collect();
        exact.extend([logical(os_home, macos), logical(target_home, macos)]);
        let mut trees: Vec<PathBuf> = [
            "/bin",
            "/sbin",
            "/usr/bin",
            "/usr/sbin",
            "/usr/lib",
            "/usr/lib64",
            "/usr/libexec",
            "/usr/share",
            "/etc",
            "/dev",
            "/proc",
            "/sys",
            "/var/db",
            "/var/root",
            "/var/run",
            "/var/log",
        ]
        .iter()
        .map(|p| logical(Path::new(p), macos))
        .collect();
        for name in [
            "bin", "sbin", "lib", "libexec", "share", "Cellar", "Caskroom", "etc",
        ] {
            trees.push(Path::new("/usr/local").join(name));
        }
        if macos {
            trees.extend(
                ["/System", "/Library", "/Applications", "/opt/homebrew"].map(PathBuf::from),
            );
        }
        for home in [os_home, target_home] {
            for name in USER_CONFIG {
                trees.push(logical(&home.join(name), macos));
            }
        }
        Self {
            exact,
            trees,
            os_home: os_home.to_path_buf(),
            target_home: target_home.to_path_buf(),
            macos,
        }
    }
    fn allow(&self, path: &Path) -> Result<(), Error> {
        if !safe(path) {
            return Err(Error::Invalid);
        }
        let path = logical(path, self.macos);
        if self.exact.contains(&path) || self.trees.iter().any(|root| path.starts_with(root)) {
            Err(Error::Forbidden)
        } else {
            Ok(())
        }
    }
}
/// `[claude] config_dir` exactly as the launch uses it (moss.rs `resolve_config_dir`):
/// trimmed, `~`/`~/…` under HOME, empty or relative values ignored.
pub fn claude_config_dir(configured: Option<&str>, home: &Path) -> Option<PathBuf> {
    let configured = configured.map(str::trim).filter(|c| !c.is_empty())?;
    let path = match configured.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            home.join(rest.trim_start_matches('/'))
        }
        _ => PathBuf::from(configured),
    };
    path.is_absolute().then_some(path)
}

const USER_CONFIG: &[&str] = &[
    ".claude", ".config", ".ssh", ".aws", ".azure", ".kube", ".gnupg", "Library",
];

fn bounded_config(path: &Path) -> Result<crate::config::Config, Error> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    const LIMIT: u64 = 1024 * 1024;
    let file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return match fs::symlink_metadata(path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    Ok(crate::config::Config::default())
                }
                _ => Err(Error::Unavailable),
            };
        }
        Err(_) => return Err(Error::Unavailable),
    };
    let metadata = file.metadata().map_err(|_| Error::Unavailable)?;
    if !metadata.is_file() || metadata.len() > LIMIT {
        return Err(Error::Unavailable);
    }
    let mut text = String::new();
    file.take(LIMIT + 1)
        .read_to_string(&mut text)
        .map_err(|_| Error::Unavailable)?;
    if text.len() as u64 > LIMIT {
        return Err(Error::Unavailable);
    }
    toml::from_str(&text).map_err(|_| Error::Unavailable)
}

fn accessible(path: &Path) -> Result<(), Error> {
    use std::os::unix::ffi::OsStrExt;
    let metadata = fs::metadata(path).map_err(fs_error)?;
    if !metadata.is_dir() {
        return Err(Error::NotDirectory);
    }
    let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| Error::Invalid)?;
    if unsafe { libc::access(path.as_ptr(), libc::R_OK | libc::X_OK) } != 0 {
        return Err(Error::Unreadable);
    }
    Ok(())
}
fn inspect(path: &Path, context: &Context) -> Result<ValidatedWorkingDirectory, Error> {
    use std::os::unix::fs::MetadataExt;
    if !safe(path) {
        return Err(Error::Invalid);
    }
    let policy = Policy::resolve(context)?;
    policy.allow(path)?;
    let canonical = path.canonicalize().map_err(fs_error)?;
    policy.allow(&canonical)?;
    accessible(&canonical)?;
    let metadata = fs::metadata(&canonical).map_err(fs_error)?;
    Ok(ValidatedWorkingDirectory {
        original: path.to_path_buf(),
        canonical,
        context: context.clone(),
        os_home: policy.os_home,
        target_home: policy.target_home,
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}
fn project(path: &Path) -> Result<PathBuf, Error> {
    if !safe(path) {
        return Err(Error::Invalid);
    }
    let mut ancestor = path.to_path_buf();
    let mut missing = Vec::new();
    loop {
        match ancestor.canonicalize() {
            Ok(mut canonical) => {
                for component in missing.iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // 存在但断开的symlink不是可创建的普通叶目录。
                if fs::symlink_metadata(&ancestor).is_ok() {
                    return Err(Error::Missing);
                }
                missing.push(ancestor.file_name().ok_or(Error::Invalid)?.to_os_string());
                if !ancestor.pop() {
                    return Err(Error::Missing);
                }
            }
            Err(error) => return Err(fs_error(error)),
        }
    }
}
fn trusted_home() -> Result<PathBuf, Error> {
    let mut storage = vec![0u8; 65536];
    let mut user = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    let rc = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            user.as_mut_ptr(),
            storage.as_mut_ptr().cast(),
            storage.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() {
        return Err(Error::Unavailable);
    }
    let user = unsafe { user.assume_init() };
    if user.pw_dir.is_null() {
        return Err(Error::Unavailable);
    }
    let home = unsafe { CStr::from_ptr(user.pw_dir) }
        .to_str()
        .map_err(|_| Error::Unavailable)?;
    let home = PathBuf::from(home);
    if !safe(&home) {
        return Err(Error::Unavailable);
    }
    Ok(home)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn working_directory_budget_keeps_worker_permits_until_real_exit() {
        use std::time::Instant;
        let active = Arc::new(AtomicUsize::new(0));
        let mut releases = Vec::new();
        for _ in 0..MAX_WORKERS {
            let (release, blocked) = mpsc::channel();
            let started = Instant::now();
            assert_eq!(
                bounded_in(active.clone(), Duration::from_millis(20), move || {
                    blocked.recv().unwrap();
                    Ok(())
                }),
                Err(Error::Unavailable)
            );
            assert!(started.elapsed() < Duration::from_secs(1));
            releases.push(release);
        }
        assert_eq!(active.load(Ordering::Acquire), MAX_WORKERS);
        for _ in 0..20 {
            assert_eq!(
                bounded_in(
                    active.clone(),
                    Duration::from_secs(2),
                    || -> Result<(), Error> { panic!("permit exhaustion must not spawn") }
                ),
                Err(Error::Unavailable)
            );
        }
        for release in releases {
            release.send(()).unwrap();
        }
        let until = Instant::now() + Duration::from_secs(1);
        while active.load(Ordering::Acquire) > 0 && Instant::now() < until {
            std::thread::yield_now();
        }
        assert_eq!(active.load(Ordering::Acquire), 0);
    }
    #[test]
    fn working_directory_permit_is_back_before_the_result_arrives() {
        // A batch checks one path after another; a worker still holding its permit after
        // answering made the third check in a row read as unavailable on a busy machine.
        let active = Arc::new(AtomicUsize::new(0));
        for n in 0..2000 {
            assert_eq!(
                bounded_in(active.clone(), Duration::from_secs(2), move || Ok(n)),
                Ok(n)
            );
            assert_eq!(active.load(Ordering::Acquire), 0, "check {n}");
        }
    }
    #[test]
    fn working_directory_test_builds_count_permits_per_thread() {
        // Another test's stuck probes must not make this thread's checks unavailable.
        let (release, blocked) = mpsc::channel::<()>();
        let blocked = Arc::new(std::sync::Mutex::new(blocked));
        let holder = std::thread::spawn(move || {
            for _ in 0..MAX_WORKERS {
                let blocked = blocked.clone();
                let held = bounded(Duration::from_millis(20), move || {
                    let _ = blocked.lock().unwrap().recv();
                    Ok(())
                });
                assert_eq!(held, Err(Error::Unavailable));
            }
            assert_eq!(
                bounded(Duration::from_millis(20), || Ok(())),
                Err(Error::Unavailable),
                "the holder's own thread is still capped"
            );
        });
        holder.join().unwrap();
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(probe_directory(temp.path(), Duration::from_secs(2)), Ok(()));
        drop(release);
    }
    #[test]
    fn working_directory_trusted_os_home_cannot_be_replaced_by_environment_context() {
        let target = tempfile::tempdir().unwrap();
        let ctx = Context::new(target.path().to_path_buf(), vec![]).unwrap();
        assert!(matches!(
            inspect(&trusted_home().unwrap(), &ctx),
            Err(Error::Forbidden)
        ));
    }
    #[test]
    fn working_directory_missing_file_permissions_and_config_context() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let work = temp.path().join("work");
        fs::create_dir(&home).unwrap();
        fs::create_dir(&work).unwrap();
        let ctx = Context::new(home, vec![]).unwrap();
        assert!(matches!(
            inspect(&temp.path().join("missing"), &ctx),
            Err(Error::Missing)
        ));
        let file = temp.path().join("file");
        fs::write(&file, "data").unwrap();
        assert!(matches!(inspect(&file, &ctx), Err(Error::NotDirectory)));
        let configured = Context::new(ctx.target_home.clone(), vec![work.clone()]).unwrap();
        assert!(matches!(inspect(&work, &configured), Err(Error::Forbidden)));
        if unsafe { libc::geteuid() } != 0 {
            fs::set_permissions(&work, fs::Permissions::from_mode(0o000)).unwrap();
            let result = inspect(&work, &ctx);
            fs::set_permissions(&work, fs::Permissions::from_mode(0o700)).unwrap();
            assert!(matches!(result, Err(Error::Unreadable)));
        }
    }
    #[test]
    fn working_directory_config_file_and_config_symlink_are_checked_in_context() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let project = temp.path().join("project");
        fs::create_dir(&home).unwrap();
        fs::create_dir(&project).unwrap();
        fs::create_dir(temp.path().join("settings")).unwrap();
        let file = temp.path().join("settings/config.toml");
        fs::write(&file, "").unwrap();
        let ctx = Context::new(home.clone(), vec![])
            .unwrap()
            .with_config_file(file.clone())
            .unwrap();
        assert!(inspect(&project, &ctx).is_ok());
        fs::write(
            &file,
            format!("[claude]\nconfig_dir = {:?}\n", project.to_string_lossy()),
        )
        .unwrap();
        assert!(matches!(inspect(&project, &ctx), Err(Error::Forbidden)));
        fs::write(&file, "").unwrap();
        std::os::unix::fs::symlink(&project, home.join(".claude")).unwrap();
        assert!(matches!(inspect(&project, &ctx), Err(Error::Forbidden)));
    }
    #[test]
    fn working_directory_claude_config_dir_follows_runtime_parsing() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let project = temp.path().join("project");
        let claude = temp.path().join("claude-config");
        for dir in [&home, &project, &claude] {
            fs::create_dir(dir).unwrap();
        }
        let file = temp.path().join("settings/config.toml");
        fs::create_dir(file.parent().unwrap()).unwrap();
        let ctx = Context::new(home, vec![])
            .unwrap()
            .with_config_file(file.clone())
            .unwrap();
        // 运行时忽略的空值与相对值不进禁区，也不能让所有启动变成 cwd_invalid。
        for ignored in ["", "   ", "relative", "~other"] {
            fs::write(&file, format!("[claude]\nconfig_dir = {ignored:?}\n")).unwrap();
            assert!(inspect(&project, &ctx).is_ok(), "{ignored:?}");
        }
        // 运行时会 trim 后使用的绝对值，按同一结果禁止该配置树。
        fs::write(
            &file,
            format!("[claude]\nconfig_dir = \" {}\"\n", claude.display()),
        )
        .unwrap();
        assert!(matches!(inspect(&claude, &ctx), Err(Error::Forbidden)));
        assert!(inspect(&project, &ctx).is_ok());
    }
    #[test]
    fn working_directory_claude_config_dir_matches_moss_resolve_table() {
        let home = Path::new("/Users/me");
        let cases = [
            (Some("~/cc"), Some("/Users/me/cc")),
            (Some("~"), Some("/Users/me")),
            (Some(" /abs "), Some("/abs")),
            (Some("/abs"), Some("/abs")),
            (Some(" "), None),
            (Some(""), None),
            (Some("relative"), None),
            (Some("~other"), None),
            (None, None),
        ];
        for (configured, expected) in cases {
            assert_eq!(
                claude_config_dir(configured, home),
                expected.map(PathBuf::from),
                "{configured:?}"
            );
        }
    }
    #[test]
    fn working_directory_config_parent_link_and_file_link_are_both_exact() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let real = temp.path().join("R");
        let visible = temp.path().join("V");
        let settings = temp.path().join("S");
        let project = temp.path().join("project");
        for dir in [&home, &real, &settings, &project] {
            fs::create_dir(dir).unwrap();
        }
        std::os::unix::fs::symlink(&real, &visible).unwrap();
        fs::write(settings.join("config.toml"), "").unwrap();
        std::os::unix::fs::symlink(settings.join("config.toml"), real.join("config.toml")).unwrap();
        let ctx = Context::new(home, vec![])
            .unwrap()
            .with_config_file(visible.join("config.toml"))
            .unwrap();
        // 可见父目录V、其真实目录R、文件链接目标的父目录S都只按Exact禁止。
        for dir in [&visible, &real, &settings] {
            assert!(
                matches!(inspect(dir, &ctx), Err(Error::Forbidden)),
                "{dir:?}"
            );
        }
        assert!(inspect(&project, &ctx).is_ok());
    }
    #[test]
    fn working_directory_custom_config_parent_is_exact_not_subtree() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let documents = home.join("Documents");
        for parent in [&home, &documents] {
            let child = parent.join("project");
            fs::create_dir_all(&child).unwrap();
            let ctx = Context::new(home.clone(), vec![])
                .unwrap()
                .with_config_file(parent.join("x.toml"))
                .unwrap();
            assert!(
                matches!(inspect(parent, &ctx), Err(Error::Forbidden)),
                "{parent:?}"
            );
            assert!(inspect(&child, &ctx).is_ok(), "{child:?}");
        }
    }
    #[test]
    fn working_directory_configuration_errors_never_become_empty_roots() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("config.toml");
        assert!(bounded_config(&file).is_ok());
        for text in ["[invalid".to_string(), "x".repeat(1024 * 1024 + 1)] {
            fs::write(&file, text).unwrap();
            assert!(matches!(bounded_config(&file), Err(Error::Unavailable)));
        }
        fs::remove_file(&file).unwrap();
        std::os::unix::fs::symlink(temp.path().join("missing"), &file).unwrap();
        assert!(matches!(bounded_config(&file), Err(Error::Unavailable)));
    }
    #[test]
    fn working_directory_policy_table_and_containers() {
        let p = Policy::table(Path::new("/Users/test"), Path::new("/target"), true);
        for path in [
            "/",
            "/Users/test",
            "/target",
            "/bin/sh",
            "/usr/share/data",
            "/private/var/db",
            "/System/Library",
            "/opt/homebrew/bin",
            "/usr/local/Cellar/pkg",
            "/target/.claude",
            "/Users/test/.ssh/key",
        ] {
            assert_eq!(p.allow(Path::new(path)), Err(Error::Forbidden), "{path}");
        }
        for path in [
            "/Users/test/Documents",
            "/Users/test/Desktop",
            "/Users/test/Downloads",
            "/opt/projects",
            "/usr/local/projects",
            "/private/tmp/work",
            "/Volumes/disk/project",
            "/Users/test/project/.claude",
            "/usr-project",
            "/target/.ssh-project",
        ] {
            assert!(p.allow(Path::new(path)).is_ok(), "{path}");
        }
    }
    #[test]
    fn working_directory_data_aliases_do_not_block_user_projects() {
        let p = Policy::table(Path::new("/Users/test"), Path::new("/target"), true);
        assert!(p
            .allow(Path::new("/System/Volumes/Data/Users/test/Documents"))
            .is_ok());
        assert_eq!(
            p.allow(Path::new("/System/Volumes/Data/private/etc")),
            Err(Error::Forbidden)
        );
        assert_eq!(
            p.allow(Path::new("/System/Volumes/Data/Users/test")),
            Err(Error::Forbidden)
        );
    }
    #[test]
    fn working_directory_symlink_context_and_recheck() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let work = temp.path().join("work");
        fs::create_dir(&home).unwrap();
        fs::create_dir(&work).unwrap();
        let ctx = Context::new(home.clone(), vec![]).unwrap();
        let checked = check(&work, &ctx).unwrap();
        recheck(&checked).unwrap();
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&home, &link).unwrap();
        assert!(matches!(check(&link, &ctx), Err(Error::Forbidden)));
        fs::remove_dir(&work).unwrap();
        std::os::unix::fs::symlink(&home, &work).unwrap();
        assert!(recheck(&checked).is_err());
    }
    #[test]
    fn working_directory_runtime_allows_exact_ancestor_not_forbidden_tree() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = Context::new(temp.path().to_path_buf(), vec![]).unwrap();
        let target = temp.path().join("state/moss");
        assert!(prepare_runtime(&target, &ctx).is_ok());
        assert!(matches!(
            prepare_runtime(&temp.path().join(".claude/moss"), &ctx),
            Err(Error::Forbidden)
        ));
        assert!(!temp.path().join(".claude").exists());
    }
}
