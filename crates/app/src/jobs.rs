use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, BufReader};
use tokio::process::ChildStderr;
use tokio::runtime::Handle;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

pub const ESCALATION: Duration = Duration::from_secs(3);
const EVICT_AFTER: Duration = Duration::from_mins(10);

pub trait Lifecycle: Clone + Send + 'static {
    fn queued() -> Self;
    fn cancelled() -> Self;
    fn is_queued(&self) -> bool;
    fn is_terminal(&self) -> bool;
}

struct Record<S> {
    state: S,
    cancel: Option<Arc<Notify>>,
    cancelled: bool,
}

pub struct Registry<K, S> {
    records: Arc<Mutex<BTreeMap<K, Record<S>>>>,
    handle: Handle,
}

impl<K, S> Clone for Registry<K, S> {
    fn clone(&self) -> Self {
        return Self { records: Arc::clone(&self.records), handle: self.handle.clone() };
    }
}

impl<K: Ord + Clone + Send + 'static, S: Lifecycle> Registry<K, S> {
    pub fn new(handle: Handle) -> Self {
        return Self { records: Arc::new(Mutex::new(BTreeMap::new())), handle };
    }

    pub fn insert(&self, key: K) -> bool {
        let Ok(mut records) = self.records.lock() else {
            return false;
        };
        if records.get(&key).is_some_and(|record| !record.state.is_terminal()) {
            return false;
        }
        records.insert(key, Record { state: S::queued(), cancel: None, cancelled: false });
        return true;
    }

    pub fn status(&self, key: &K) -> Option<S> {
        return self.records.lock().ok()?.get(key).map(|record| record.state.clone());
    }

    pub fn snapshot(&self) -> Vec<(K, S)> {
        let Ok(records) = self.records.lock() else {
            return Vec::new();
        };
        return records.iter().map(|(key, record)| (key.clone(), record.state.clone())).collect();
    }

    pub fn update(&self, key: &K, apply: impl FnOnce(&mut S)) {
        if let Ok(mut records) = self.records.lock()
            && let Some(record) = records.get_mut(key)
        {
            apply(&mut record.state);
        }
    }

    pub fn set_state(&self, key: &K, state: S) {
        self.update(key, |current| *current = state);
    }

    pub fn finish(&self, key: &K, state: S) {
        self.set_state(key, state);
        self.evict(key);
    }

    pub fn register_cancel(&self, key: &K, notify: Arc<Notify>) {
        if let Ok(mut records) = self.records.lock()
            && let Some(record) = records.get_mut(key)
        {
            record.cancel = Some(notify);
        }
    }

    pub fn take_cancelled(&self, key: &K) -> bool {
        let Ok(mut records) = self.records.lock() else {
            return true;
        };
        let Some(record) = records.get_mut(key) else {
            return true;
        };
        if record.cancelled {
            record.state = S::cancelled();
            return true;
        }
        return false;
    }

    pub fn cancel(&self, key: &K) {
        let Ok(mut records) = self.records.lock() else {
            return;
        };
        let Some(record) = records.get_mut(key) else {
            return;
        };
        record.cancelled = true;
        if let Some(notify) = &record.cancel {
            notify.notify_one();
        }
        if record.state.is_queued() {
            record.state = S::cancelled();
            self.evict(key);
        }
    }

    pub fn evict(&self, key: &K) {
        let records = Arc::clone(&self.records);
        let key = key.clone();
        self.handle.spawn(async move {
            tokio::time::sleep(EVICT_AFTER).await;
            if let Ok(mut records) = records.lock()
                && records.get(&key).is_none_or(|record| record.state.is_terminal())
            {
                records.remove(&key);
            }
        });
    }
}

pub fn drain_stderr(handle: &Handle, stderr: Option<ChildStderr>) -> Option<JoinHandle<String>> {
    return stderr.map(|stderr| {
        handle.spawn(async move {
            let mut text = String::new();
            let mut reader = BufReader::new(stderr);
            let _ = reader.read_to_string(&mut text).await;
            return text;
        })
    });
}

pub async fn join_stderr(task: Option<JoinHandle<String>>) -> String {
    return match task {
        Some(task) => task.await.unwrap_or_default(),
        None => String::new(),
    };
}

pub fn failure_reason(stderr: &str, tool: &str) -> String {
    for line in stderr.lines().rev() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    return format!("{tool} failed");
}
