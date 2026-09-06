use super::{
    domain::{GithubRequest, GithubResponse, GithubResult},
    service::GithubService,
    ResolvedGithubScope,
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
const WORKERS: usize = 4;
const CACHE_TTL: Duration = Duration::from_secs(15);
const CACHE_CAPACITY: usize = 64;

struct Job {
    id: u64,
    scope: ResolvedGithubScope,
    request: GithubRequest,
    refresh: bool,
    cancelled: Arc<AtomicBool>,
}
struct Completion {
    id: u64,
    result: GithubResult<GithubResponse>,
}
struct Pending {
    cancelled: Arc<AtomicBool>,
    result: Option<GithubResult<GithubResponse>>,
}
struct Cached {
    inserted: Instant,
    response: GithubResponse,
}
#[derive(Default)]
struct Cache {
    epoch: u64,
    entries: HashMap<String, Cached>,
}

pub struct GithubRuntime {
    sender: Option<SyncSender<Job>>,
    receiver: Option<Receiver<Completion>>,
    pending: HashMap<u64, Pending>,
    program: std::path::PathBuf,
}
impl Default for GithubRuntime {
    fn default() -> Self {
        Self::with_program(std::path::PathBuf::from("gh"))
    }
}
impl GithubRuntime {
    pub fn with_program(program: std::path::PathBuf) -> Self {
        Self {
            sender: None,
            receiver: None,
            pending: HashMap::new(),
            program,
        }
    }
    fn start_workers(&mut self) {
        let (sender, jobs) = mpsc::sync_channel::<Job>(32);
        let (completed, receiver) = mpsc::channel();
        let jobs = Arc::new(Mutex::new(jobs));
        let cache = Arc::new(Mutex::new(Cache::default()));
        for _ in 0..WORKERS {
            let jobs = Arc::clone(&jobs);
            let completed = completed.clone();
            let cache = Arc::clone(&cache);
            let program = self.program.clone();
            thread::spawn(move || loop {
                let job = match jobs.lock() {
                    Ok(receiver) => match receiver.recv() {
                        Ok(job) => job,
                        Err(_) => break,
                    },
                    Err(_) => break,
                };
                if job.cancelled.load(Ordering::Relaxed) {
                    continue;
                }
                let mutation = matches!(job.request, GithubRequest::Mutate(_));
                let key = serde_json::to_string(&job.request)
                    .map(|request| format!("{:?}:{request}", job.scope));
                let mut epoch = None;
                let mut cached = None;
                if let Ok(mut cache) = cache.lock() {
                    if mutation || job.refresh {
                        cache.epoch = cache.epoch.wrapping_add(1);
                        cache.entries.clear();
                    }
                    epoch = Some(cache.epoch);
                    if !mutation && !job.refresh {
                        if let Ok(key) = &key {
                            cached = cache
                                .entries
                                .get(key)
                                .filter(|entry| entry.inserted.elapsed() < CACHE_TTL)
                                .map(|entry| entry.response.clone());
                        }
                    }
                }
                let cache_hit = cached.is_some();
                let result = match cached {
                    Some(response) => Ok(response),
                    None => GithubService {
                        scope: Some(job.scope),
                        cancelled: Arc::clone(&job.cancelled),
                        program: program.clone(),
                    }
                    .execute(&job.request),
                };
                if let Ok(mut cache) = cache.lock() {
                    if mutation {
                        // A failed or cancelled write may still have reached GitHub.
                        cache.epoch = cache.epoch.wrapping_add(1);
                        cache.entries.clear();
                    } else if !cache_hit
                        && epoch == Some(cache.epoch)
                        && !job.cancelled.load(Ordering::Relaxed)
                    {
                        if let (Ok(key), Ok(response)) = (key, &result) {
                            cache
                                .entries
                                .retain(|_, entry| entry.inserted.elapsed() < CACHE_TTL);
                            if cache.entries.len() >= CACHE_CAPACITY {
                                if let Some(oldest) = cache
                                    .entries
                                    .iter()
                                    .min_by_key(|(_, entry)| entry.inserted)
                                    .map(|(key, _)| key.clone())
                                {
                                    cache.entries.remove(&oldest);
                                }
                            }
                            cache.entries.insert(
                                key,
                                Cached {
                                    inserted: Instant::now(),
                                    response: response.clone(),
                                },
                            );
                        }
                    }
                }
                if !job.cancelled.load(Ordering::Relaxed)
                    && completed.send(Completion { id: job.id, result }).is_err()
                {
                    break;
                }
            });
        }
        self.sender = Some(sender);
        self.receiver = Some(receiver);
    }
}
impl GithubRuntime {
    pub fn submit(&mut self, scope: ResolvedGithubScope, request: GithubRequest) -> u64 {
        self.enqueue(scope, request, false)
    }
    pub fn submit_refresh(&mut self, scope: ResolvedGithubScope, request: GithubRequest) -> u64 {
        self.enqueue(scope, request, true)
    }
    fn enqueue(
        &mut self,
        scope: ResolvedGithubScope,
        request: GithubRequest,
        refresh: bool,
    ) -> u64 {
        if self.sender.is_none() {
            self.start_workers();
        }
        let id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let cancelled = Arc::new(AtomicBool::new(false));
        let job = Job {
            id,
            scope,
            request,
            refresh,
            cancelled: Arc::clone(&cancelled),
        };
        let result = match &self.sender {
            Some(sender) => match sender.try_send(job) {
                Ok(()) => None,
                Err(TrySendError::Full(_)) => Some(Err(
                    "GitHub is busy. Try again after current requests finish.".into(),
                )),
                Err(TrySendError::Disconnected(_)) => {
                    Some(Err("GitHub workers are unavailable".into()))
                }
            },
            None => Some(Err("GitHub runtime is closed".into())),
        };
        self.pending.insert(id, Pending { cancelled, result });
        id
    }
    pub fn cancel(&mut self, id: u64) {
        if let Some(pending) = self.pending.remove(&id) {
            pending.cancelled.store(true, Ordering::Relaxed);
        }
    }
    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        let Some(receiver) = &self.receiver else {
            return false;
        };
        while let Ok(completion) = receiver.try_recv() {
            if let Some(pending) = self.pending.get_mut(&completion.id) {
                pending.result = Some(completion.result);
                changed = true;
            }
        }
        changed
    }
    pub fn take(&mut self, id: u64) -> Option<GithubResult<GithubResponse>> {
        self.pending.get(&id)?.result.as_ref()?;
        self.pending.remove(&id)?.result
    }
    pub fn has_pending(&self) -> bool {
        self.pending
            .values()
            .any(|pending| pending.result.is_none())
    }
}
impl Drop for GithubRuntime {
    fn drop(&mut self) {
        for pending in self.pending.values() {
            pending.cancelled.store(true, Ordering::Relaxed);
        }
        self.sender.take();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn cancelling_a_request_terminates_its_process_without_blocking_other_requests() {
        let root = std::env::temp_dir().join(format!(
            "gardn-github-cancel-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("fixture directory");
        let program = root.join("gh");
        let block = root.join("gh.block");
        let pid_file = root.join("gh.pid");
        std::fs::write(&block, "").expect("block first request");
        std::fs::write(&program, "#!/bin/sh\nif test -f \"$0.block\"; then\n  printf '%s' \"$$\" > \"$0.pid\"\n  exec sleep 30\nfi\nprintf '%s' '{\"login\":\"fixture-viewer\"}'\n").expect("fixture executable");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))
            .expect("executable permissions");
        let mut runtime = GithubRuntime::with_program(program);
        let scope = ResolvedGithubScope {
            repositories: Vec::new(),
            organization: None,
        };
        let request = runtime.submit(scope.clone(), GithubRequest::Viewer);
        let deadline = Instant::now() + Duration::from_secs(5);
        let pid: u32 = loop {
            if let Some(pid) = std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|value| value.parse().ok())
            {
                break pid;
            }
            assert!(Instant::now() < deadline, "GitHub process did not start");
            thread::sleep(Duration::from_millis(5));
        };
        runtime.cancel(request);
        while crate::platform::process_exists(pid) {
            assert!(
                Instant::now() < deadline,
                "cancelled GitHub process is still running"
            );
            thread::sleep(Duration::from_millis(5));
        }
        std::fs::remove_file(block).expect("allow next request");
        let next = runtime.submit(scope, GithubRequest::Viewer);
        loop {
            runtime.tick();
            if let Some(result) = runtime.take(next) {
                let GithubResponse::Viewer(viewer) = result.expect("next request succeeds") else {
                    panic!("expected viewer");
                };
                assert_eq!(viewer.login, "fixture-viewer");
                break;
            }
            assert!(Instant::now() < deadline, "next GitHub request was blocked");
            thread::sleep(Duration::from_millis(5));
        }
        std::fs::remove_dir_all(root).expect("remove fixture");
    }
}
