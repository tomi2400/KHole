//! The background analysis queue: a few worker threads take the open session's tracks
//! that lack a BPM or key, run the sidecar on them, and cache the results by `file_key`.
//!
//! One session is analysed at a time (the open one). Enqueueing it again (after a rescan)
//! only adds what is new; enqueueing another session, or [`Analyzer::stop`], drops the
//! queue and cancels the running analyses.

use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{Error, Result};
use crate::store::analysis::{self as cache, Job};
use crate::store::{self, Store, tracks};

/// What the queue needs from the app; tests provide their own.
pub trait Host: Send + Sync + 'static {
    fn store(&self) -> &Store;
    fn sidecar(&self) -> Result<Command>;
    fn progress(&self, p: Progress);
}

impl Host for AppHandle {
    fn store(&self) -> &Store {
        self.state::<Store>().inner()
    }
    fn sidecar(&self) -> Result<Command> {
        super::sidecar_command(self)
    }
    fn progress(&self, p: Progress) {
        let _ = self.emit("analysis_progress", p);
    }
}

/// `analysis_progress`: `done` of `total` tracks of this round are analysed. A round ends
/// when `done == total`; the next enqueue starts a new one from zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub session_id: i64,
    pub done: usize,
    pub total: usize,
}

#[derive(Default)]
struct State {
    session: Option<i64>,
    /// Bumped whenever the queue is dropped; a running job of an older generation is
    /// cancelled.
    generation: u64,
    queue: VecDeque<Job>,
    /// Track ids queued or running in this generation, so a rescan does not add them twice.
    known: HashSet<i64>,
    running: usize,
    done: usize,
    total: usize,
    workers_started: bool,
    version: Option<String>,
}

impl State {
    fn progress(&self) -> Option<Progress> {
        self.session.map(|session_id| Progress {
            session_id,
            done: self.done,
            total: self.total,
        })
    }
}

struct Inner<H> {
    host: H,
    state: Mutex<State>,
    work: Condvar,
}

impl<H> Inner<H> {
    fn lock(&self) -> Result<MutexGuard<'_, State>> {
        self.state.lock().map_err(|_| Error::StorePoisoned)
    }
}

pub struct Analyzer<H: Host = AppHandle> {
    inner: Arc<Inner<H>>,
}

/// Half the cores, at most four: the sidecar is single-threaded, and the waveform of the
/// track being listened to must still decode quickly.
pub(crate) fn worker_count() -> usize {
    std::thread::available_parallelism()
        .map_or(2, |n| n.get() / 2)
        .clamp(1, 4)
}

impl<H: Host> Analyzer<H> {
    pub fn new(host: H) -> Self {
        Self {
            inner: Arc::new(Inner {
                host,
                state: Mutex::new(State::default()),
                work: Condvar::new(),
            }),
        }
    }

    /// The sidecar's version (asked once); results of another version are redone.
    fn version(&self) -> Result<String> {
        if let Some(v) = &self.inner.lock()?.version {
            return Ok(v.clone());
        }
        let v = super::sidecar_info(self.inner.host.sidecar()?)?.version;
        self.inner.lock()?.version = Some(v.clone());
        Ok(v)
    }

    /// Queues the session's tracks that need analysing (see [`cache::wanted`]). Blocking
    /// (it may ask the sidecar its version). Returns the progress of the round.
    pub fn enqueue(&self, session_id: i64) -> Result<Progress> {
        let version = self.version()?;
        let jobs = cache::wanted(&*self.inner.host.store().conn()?, session_id, &version)?;
        let progress = {
            let mut st = self.inner.lock()?;
            if st.session != Some(session_id) {
                reset(&mut st);
                st.session = Some(session_id);
            }
            for job in jobs {
                if st.known.insert(job.track_id) {
                    st.queue.push_back(job);
                    st.total += 1;
                }
            }
            if !st.queue.is_empty() && !st.workers_started {
                st.workers_started = true;
                for _ in 0..worker_count() {
                    let inner = Arc::clone(&self.inner);
                    std::thread::Builder::new()
                        .name("analysis".into())
                        .spawn(move || worker(&inner))?;
                }
            }
            self.inner.work.notify_all();
            st.progress().expect("session set")
        };
        self.inner.host.progress(progress);
        Ok(progress)
    }

    /// [`Self::stop`], if `session_id` is the one being analysed.
    pub fn stop_session(&self, session_id: i64) {
        let current = self.inner.lock().map(|st| st.session);
        if current.is_ok_and(|s| s == Some(session_id)) {
            self.stop();
        }
    }

    /// Drops the queue and cancels the running analyses (the session was closed).
    pub fn stop(&self) {
        if let Ok(mut st) = self.inner.lock() {
            reset(&mut st);
            st.session = None;
        }
    }
}

fn reset(st: &mut State) {
    st.generation += 1;
    st.queue.clear();
    st.known.clear();
    st.done = 0;
    st.total = 0;
}

fn worker<H: Host>(inner: &Inner<H>) {
    background_qos();
    loop {
        let Ok(mut st) = inner.lock() else { return };
        let job = loop {
            match st.queue.pop_front() {
                Some(job) => break job,
                None => match inner.work.wait(st) {
                    Ok(guard) => st = guard,
                    Err(_) => return,
                },
            }
        };
        st.running += 1;
        let generation = st.generation;
        let version = st.version.clone().unwrap_or_default();
        drop(st);

        let cancelled = || inner.lock().map_or(true, |s| s.generation != generation);
        run_job(&inner.host, &job, &version, cancelled);

        let Ok(mut st) = inner.lock() else { return };
        st.running -= 1;
        if st.generation != generation {
            continue;
        }
        st.known.remove(&job.track_id);
        st.done += 1;
        let progress = st.progress();
        if st.done == st.total {
            // Round over: the next enqueue counts from zero.
            st.done = 0;
            st.total = 0;
        }
        drop(st);
        if let Some(p) = progress {
            inner.host.progress(p);
        }
    }
}

/// Analyses one file and caches the outcome. A failure that is the file's own (cannot be
/// decoded, too short, the analyzer gave up) is cached so it is not retried; one that is
/// not (file gone meanwhile, sidecar missing) is not.
fn run_job<H: Host>(host: &H, job: &Job, version: &str, cancelled: impl Fn() -> bool) {
    let store = host.store();
    // The track may have been moved or edited since it was queued.
    let (path, file_key) = {
        let Ok(conn) = store.conn() else { return };
        let (Ok(path), Ok(key)) = (
            tracks::path(&conn, job.track_id),
            tracks::file_key(&conn, job.track_id),
        ) else {
            return;
        };
        if cache::has_current(&conn, &key, version).unwrap_or(false) {
            return; // a copy of the same file was done meanwhile
        }
        (path, key)
    };
    let cmd = match host.sidecar() {
        Ok(cmd) => cmd,
        Err(e) => {
            eprintln!("analysis: {e}");
            return;
        }
    };
    let outcome = match super::analyze(cmd, Path::new(&path), cancelled) {
        Ok(a) => Ok(a),
        Err(e @ (Error::Decode(_) | Error::Sidecar(_))) => Err(e.to_string()),
        Err(Error::Cancelled) => return,
        Err(e) => {
            eprintln!("analysis of {path} skipped: {e}");
            return;
        }
    };
    if let Ok(conn) = store.conn() {
        let _ = cache::put(
            &conn,
            &file_key,
            outcome.as_ref().map_err(String::as_str),
            version,
            store::now(),
        );
    }
}

/// Worker threads decode at utility QoS: below the UI and the waveform request.
fn background_qos() {
    // SAFETY: sets the calling thread's own QoS class; no memory is involved.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::tests::built_sidecar;
    use crate::library::tags::TrackMeta;
    use crate::library::testutil::{write_click_wav, write_wav};
    use crate::store::sessions;
    use std::sync::mpsc;
    use std::time::Duration;

    struct TestHost {
        store: Store,
        events: Mutex<mpsc::Sender<Progress>>,
    }

    impl Host for TestHost {
        fn store(&self) -> &Store {
            &self.store
        }
        fn sidecar(&self) -> Result<Command> {
            Ok(built_sidecar().expect("checked by the test"))
        }
        fn progress(&self, p: Progress) {
            let _ = self.events.lock().unwrap().send(p);
        }
    }

    #[test]
    fn analyses_a_session_once() {
        if built_sidecar().is_none() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let mut rows = Vec::new();
        for (name, bpm) in [("a.wav", 124.0), ("b.wav", 132.0)] {
            let p = dir.path().join(name);
            write_click_wav(&p, 20.0, bpm);
            rows.push(p);
        }
        let short = dir.path().join("c.wav");
        write_wav(&short, 1.0, None);
        rows.push(short);

        let (tx, rx) = mpsc::channel();
        let host = TestHost {
            store: Store::open_in_memory().unwrap(),
            events: Mutex::new(tx),
        };
        let session = {
            let mut conn = host.store.conn().unwrap();
            let s = sessions::open(&conn, dir.path().to_str().unwrap(), 0).unwrap();
            let new: Vec<_> = rows
                .iter()
                .map(|p| tracks::NewTrack {
                    path: p.to_str().unwrap().into(),
                    file_key: crate::library::file_key::file_key(p).unwrap(),
                    meta: TrackMeta {
                        format: "WAV".into(),
                        ..TrackMeta::default()
                    },
                })
                .collect();
            tracks::upsert(&mut conn, s.id, &new).unwrap();
            s.id
        };

        let analyzer = Analyzer::new(host);
        let first = analyzer.enqueue(session).unwrap();
        assert_eq!((first.done, first.total), (0, 3));
        // Enqueueing again while it runs adds nothing.
        assert_eq!(analyzer.enqueue(session).unwrap().total, 3);
        loop {
            let p = rx.recv_timeout(Duration::from_secs(60)).unwrap();
            if p.total > 0 && p.done == p.total {
                break;
            }
        }

        let list = tracks::list(&analyzer.inner.host.store.conn().unwrap(), session).unwrap();
        let bpms: Vec<_> = list.iter().map(|t| t.bpm).collect();
        assert!((bpms[0].unwrap() - 124.0).abs() < 0.15, "{bpms:?}");
        assert!((bpms[1].unwrap() - 132.0).abs() < 0.15, "{bpms:?}");
        assert_eq!(bpms[2], None, "too short: cached as a failure");
        assert!(list[0].bpm_source == Some("analysis") && list[0].key_analyzed);

        // Everything is cached now (the failure too): nothing to do.
        let again = analyzer.enqueue(session).unwrap();
        assert_eq!((again.done, again.total), (0, 0));
    }
}
