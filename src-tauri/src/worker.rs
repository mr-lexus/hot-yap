use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{mpsc, Mutex};

use crate::state::{self, AppState};

/// One message from the worker (always carries an id when it answers a request).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerMessage {
    pub id: Option<u64>,
    pub ok: Option<bool>,
    pub event: Option<String>,
    pub error: Option<String>,
    #[serde(flatten)]
    pub payload: Value,
}

pub struct Worker {
    pub alive: AtomicBool,
    generation: AtomicU64,
    child: Mutex<Option<Child>>,
    /// OS process id of the spawned worker. Kept separately from `child`
    /// because `spawn_watcher` takes the `Child` out of `worker.child` to
    /// wait on it, which used to make `kill()` a no-op (restart could never
    /// actually terminate a hung worker, so the old process kept holding the
    /// model + VRAM while a fresh one failed to answer). Killed via
    /// `taskkill /T` on Windows so the whole PyInstaller tree goes down.
    pid: StdMutex<Option<u32>>,
    stdin: Mutex<Option<ChildStdin>>,
    pending: StdMutex<HashMap<u64, mpsc::UnboundedSender<WorkerMessage>>>,
    next_id: AtomicU64,
}

struct WorkerLaunch {
    program: PathBuf,
    script: Option<PathBuf>,
}

/// Events that terminate a request (everything else is treated as progress).
fn is_terminal(msg: &WorkerMessage) -> bool {
    if msg.ok == Some(false) {
        return true;
    }
    matches!(
        msg.event.as_deref(),
        Some(
            "status"
                | "model_downloaded"
                | "model_loaded"
                | "transcribed"
                | "shutdown_ack"
                | "cuda_runtime_verified"
                | "cuda_runtime_downloaded"
        )
    )
}

pub fn python_path() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("VOXSHIFT_PYTHON") {
        return Ok(PathBuf::from(p));
    }
    let interpreter = if cfg!(windows) {
        "Scripts/python.exe"
    } else {
        "bin/python"
    };
    let venv = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../backend/.venv")
        .join(interpreter);
    if venv.exists() {
        return Ok(venv);
    }
    Err(format!(
        "Python venv not found at {} — run ./scripts/bootstrap.sh first",
        venv.display()
    ))
}

fn worker_script() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../backend/worker.py")
}

fn worker_launch() -> Result<WorkerLaunch, String> {
    if let Ok(path) = std::env::var("VOXSHIFT_WORKER") {
        return Ok(WorkerLaunch {
            program: PathBuf::from(path),
            script: None,
        });
    }

    if std::env::var_os("VOXSHIFT_PYTHON").is_some() {
        return Ok(WorkerLaunch {
            program: python_path()?,
            script: Some(worker_script()),
        });
    }

    let executable_name = if cfg!(windows) {
        "hotyap-worker.exe"
    } else {
        "hotyap-worker"
    };
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join(executable_name)));
    if let Some(program) = bundled.filter(|path| path.is_file()) {
        return Ok(WorkerLaunch {
            program,
            script: None,
        });
    }

    Ok(WorkerLaunch {
        program: python_path()?,
        script: Some(worker_script()),
    })
}

pub fn is_alive(app: &AppHandle) -> bool {
    app.try_state::<Arc<Worker>>()
        .map(|w| w.alive.load(Ordering::SeqCst))
        .unwrap_or(false)
}

/// Spawn the Python worker process and wire up stdin/stdout/stderr handling.
pub async fn start(app: &AppHandle) -> Result<(), String> {
    let launch = worker_launch()?;
    let mut command = tokio::process::Command::new(&launch.program);
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW: the worker is a console-subsystem process
        // (python.exe in dev, or the PyInstaller sidecar in release).
        // Spawned from a GUI parent without flags it would allocate a
        // visible terminal window; this keeps it running in the background.
        // The piped stdin/stdout/stderr are unaffected, so the JSONL
        // protocol still works.
        command.creation_flags(0x0800_0000);
    }
    if let Some(script) = &launch.script {
        command.arg(script);
    }
    let child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Failed to start worker ({:?}): {e}", launch.program))?;

    if let Some(script) = &launch.script {
        log::info!(
            "worker started: {} {}",
            launch.program.display(),
            script.display()
        );
    } else {
        log::info!("bundled worker started: {}", launch.program.display());
    }
    let worker = match app.try_state::<Arc<Worker>>() {
        Some(existing) => existing.inner().clone(),
        None => {
            let worker = Arc::new(Worker {
                alive: AtomicBool::new(true),
                generation: AtomicU64::new(0),
                child: Mutex::new(None),
                pid: StdMutex::new(None),
                stdin: Mutex::new(None),
                pending: StdMutex::new(HashMap::new()),
                next_id: AtomicU64::new(1),
            });
            app.manage(worker.clone());
            worker
        }
    };
    worker.alive.store(true, Ordering::SeqCst);
    worker.pending.lock().unwrap().clear();
    *worker.pid.lock().unwrap() = child.id();
    *worker.stdin.lock().await = None;
    // A previous watcher may still hold the child lock if its process is
    // wedged and refuses to die; never hang a restart on that.
    match tokio::time::timeout(Duration::from_secs(5), worker.child.lock()).await {
        Ok(mut guard) => {
            *guard = Some(child);
        }
        Err(_) => {
            let _ = kill(app).await;
            return Err(
                "previous worker process did not die; cannot start a new one".to_string(),
            );
        }
    }
    let generation = worker.generation.fetch_add(1, Ordering::SeqCst) + 1;

    let (stdout, stderr, stdin) = {
        let mut child = worker.child.lock().await;
        let child = child.as_mut().unwrap();
        (child.stdout.take(), child.stderr.take(), child.stdin.take())
    };
    *worker.stdin.lock().await = stdin;

    match (stdout, stderr) {
        (Some(out), Some(err)) if worker.stdin.lock().await.is_some() => {
            spawn_stdout_reader(worker.clone(), out);
            spawn_stderr_reader(err);
            spawn_watcher(app.clone(), worker.clone(), generation);
        }
        _ => {
            let _ = kill(app).await;
            return Err("Failed to open worker stdio pipes".to_string());
        }
    }

    // Verify the worker answers.
    let resp = request(
        app,
        &worker,
        json!({"command": "status", "model_dir": model_dir(app)}),
        Duration::from_secs(15),
    )
    .await;
    match resp {
        Ok(msg) => {
            log::info!("worker ready: {:?}", msg.event);
            Ok(())
        }
        Err(e) => {
            let _ = kill(app).await;
            Err(e)
        }
    }
}

fn spawn_stdout_reader(worker: Arc<Worker>, stdout: tokio::process::ChildStdout) {
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut read_error = None;
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<WorkerMessage>(line) {
                        Ok(msg) => {
                            if let Some(id) = msg.id {
                                let mut pending = worker.pending.lock().unwrap();
                                if let Some(tx) = pending.get(&id) {
                                    let _ = tx.send(msg.clone());
                                    if is_terminal(&msg) {
                                        pending.remove(&id);
                                    }
                                }
                            }
                        }
                        Err(e) => log::warn!("non-protocol line from worker stdout: {e}"),
                    }
                }
                Ok(None) => break, // EOF: worker closed stdout or exited
                Err(e) => {
                    read_error = Some(e);
                    break;
                }
            }
        }
        if let Some(e) = read_error {
            log::warn!("worker stdout read error: {e}");
        }
        log::info!("worker stdout closed");
        // The worker's stdout is gone: the protocol is dead even if the
        // process itself is still alive (observed with a PyInstaller onefile
        // worker spawned from a GUI parent: python keeps running and printed
        // its reply, but the pipe broke). Fail every pending request right
        // away instead of letting the caller spin until its timeout.
        worker.alive.store(false, Ordering::SeqCst);
        let mut pending = worker.pending.lock().unwrap();
        for (_, tx) in pending.drain() {
            let _ = tx.send(WorkerMessage {
                id: None,
                ok: Some(false),
                event: None,
                error: Some("Worker closed the connection".to_string()),
                payload: json!({}),
            });
        }
    });
}

fn spawn_stderr_reader(stderr: tokio::process::ChildStderr) {
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            log::info!("[worker] {line}");
        }
    });
}

fn spawn_watcher(app: AppHandle, worker: Arc<Worker>, generation: u64) {
    tauri::async_runtime::spawn(async move {
        // Take the child out and drop the lock BEFORE waiting. Holding the
        // mutex across wait() would block any other worker.child.lock() —
        // including kill() and therefore app shutdown — for the entire
        // lifetime of the worker process.
        let child = {
            let mut guard = worker.child.lock().await;
            guard.take()
        };
        let status = match child {
            Some(mut c) => Some(c.wait().await),
            None => None,
        };
        if status.is_none() {
            return;
        }
        if worker.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        log::info!("worker process exited: {:?}", status);
        worker.alive.store(false, Ordering::SeqCst);
        {
            let mut pending = worker.pending.lock().unwrap();
            pending.clear(); // dropping senders resolves pending requests with None
        }
        state::on_worker_exit(&app);
    });
}

/// Send a request and await the terminal response. Download progress events
/// are relayed to the UI as `vox:download-progress`.
pub async fn request(
    app: &AppHandle,
    worker: &Worker,
    command: Value,
    timeout: Duration,
) -> Result<WorkerMessage, String> {
    if !worker.alive.load(Ordering::SeqCst) {
        return Err("Python worker is not running".to_string());
    }
    let id = worker.next_id.fetch_add(1, Ordering::SeqCst);
    let (tx, mut rx) = mpsc::unbounded_channel();
    worker.pending.lock().unwrap().insert(id, tx);

    let mut line = json!({"id": id});
    if let Value::Object(map) = &mut line {
        if let Value::Object(cmd) = command {
            map.extend(cmd);
        }
    }
    log::debug!("-> worker {line}");

    let write_res = {
        let stdin_opt = worker.stdin.lock().await.take();
        match stdin_opt {
            Some(mut s) => {
                let mut buf = line.to_string();
                buf.push('\n');
                // Writing must be bounded too: if the worker stopped reading
                // stdin (e.g. stuck in interpreter teardown after
                // shutdown_ack), a full pipe buffer would block this request
                // forever, past every response timeout.
                let write_timeout = timeout.min(Duration::from_secs(5));
                let res = match tokio::time::timeout(write_timeout, async {
                    s.write_all(buf.as_bytes()).await?;
                    s.flush().await
                })
                .await
                {
                    Ok(r) => r,
                    Err(_) => Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "timed out writing to worker",
                    )),
                };
                *worker.stdin.lock().await = Some(s);
                res
            }
            None => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "worker stdin is closed",
            )),
        }
    };
    if let Err(e) = write_res {
        worker.pending.lock().unwrap().remove(&id);
        return Err(format!("Failed to write to worker: {e}"));
    }

    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let msg = match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(m)) => m,
            Ok(None) => return Err("Worker closed the connection".to_string()),
            Err(_) => {
                return Err(format!(
                    "Worker did not answer within {}s",
                    timeout.as_secs()
                ))
            }
        };
        if is_terminal(&msg) {
            if msg.ok == Some(false) {
                return Err(msg
                    .error
                    .or_else(|| {
                        msg.payload
                            .get("error")
                            .and_then(|value| value.as_str())
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| "Worker command failed".to_string()));
            }
            return Ok(msg);
        }
        if msg.event.as_deref() == Some("download_progress") {
            if let Some(f) = msg.payload.get("fraction").and_then(|v| v.as_f64()) {
                if let Some(st) = app.try_state::<AppState>() {
                    st.lock().model_progress = Some(f as f32);
                }
                let _ = app.emit("vox:download-progress", json!({"fraction": f}));
            }
        }
        if msg.event.as_deref() == Some("transcribe_progress") {
            if let Some(e) = msg.payload.get("elapsed").and_then(|v| v.as_f64()) {
                let _ = app.emit("vox:transcribe-progress", json!({
                    "elapsed": e,
                    "fraction": msg.payload.get("fraction").and_then(|v| v.as_f64())
                }));
            }
        }
        if msg.event.as_deref() == Some("cuda_runtime_progress") {
            if let Some(f) = msg.payload.get("fraction").and_then(|v| v.as_f64()) {
                if let Some(st) = app.try_state::<AppState>() {
                    st.lock().cuda_runtime.progress = Some(f as f32);
                }
                let _ = app.emit("vox:cuda-runtime-progress", json!({"fraction": f}));
            }
        }
    }
}

/// Ask the worker to shut down, wait briefly, then kill it if needed.
/// Bounded: every step has a timeout so app shutdown can never wedge here.
pub async fn shutdown(app: &AppHandle) {
    let worker = match app.try_state::<Arc<Worker>>() {
        Some(w) => w,
        None => return,
    };
    if worker.alive.load(Ordering::SeqCst) {
        let _ = request(
            app,
            &worker,
            json!({"command": "shutdown"}),
            Duration::from_secs(3),
        )
        .await;
    }
    let _ = tokio::time::timeout(Duration::from_secs(10), kill(app)).await;
    log::info!("worker shutdown complete");
}

pub async fn kill(app: &AppHandle) -> Result<(), String> {
    let worker = match app.try_state::<Arc<Worker>>() {
        Some(w) => w.clone(),
        None => return Ok(()),
    };
    worker.alive.store(false, Ordering::SeqCst);
    {
        let mut pending = worker.pending.lock().unwrap();
        pending.clear();
    }
    // Kill by OS pid first: `spawn_watcher` may have taken the `Child` out of
    // `worker.child` to wait on it, so the handle below is not always
    // available. taskkill /T terminates the whole PyInstaller tree
    // (bootloader + python child), which a plain child.kill() would leave
    // orphaned and holding the model + VRAM.
    let pid = worker.pid.lock().unwrap().take();
    if let Some(pid) = pid {
        let mut command = if cfg!(windows) {
            let mut c = std::process::Command::new("taskkill");
            c.args(["/F", "/T", "/PID", &pid.to_string()]);
            // CREATE_NO_WINDOW: this short call would otherwise flash a
            // terminal window every time the worker is killed/restarted.
            #[cfg(windows)]
            c.creation_flags(0x0800_0000);
            c
        } else {
            let mut c = std::process::Command::new("kill");
            c.args(["-9", &pid.to_string()]);
            c
        };
        // taskkill is a short synchronous call; run it off the async runtime.
        match tokio::task::spawn_blocking(move || command.status()).await {
            Ok(Ok(status)) if status.success() => log::debug!("worker pid {pid} terminated"),
            Ok(Ok(_)) => log::debug!("worker pid {pid} already gone"),
            Ok(Err(e)) => log::warn!("failed to kill worker pid {pid}: {e}"),
            Err(e) => log::warn!("kill task failed for worker pid {pid}: {e}"),
        }
    }
    // The watcher normally drops the child lock as soon as the process dies,
    // but if the process refuses to die (wedged CUDA/CTranslate2 teardown,
    // stale pid, ...) kill() must not block app shutdown forever.
    match tokio::time::timeout(Duration::from_secs(5), worker.child.lock()).await {
        Ok(mut guard) => {
            if let Some(mut c) = guard.take() {
                let _ = c.kill().await;
                let _ = tokio::time::timeout(Duration::from_secs(5), c.wait()).await;
            }
        }
        Err(_) => {
            log::warn!("worker child lock timed out in kill(); watcher still owns it");
        }
    }
    Ok(())
}

pub fn model_dir(app: &AppHandle) -> PathBuf {
    let st = app.state::<AppState>();
    let inner = st.lock();
    inner.model_dir.clone()
}
