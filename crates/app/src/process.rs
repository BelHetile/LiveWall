use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::process::{ExitStatus, Output, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use tokio::process::{ChildStderr, ChildStdout, Command};

static LIVE: Mutex<BTreeMap<u32, i32>> = Mutex::new(BTreeMap::new());

pub fn kill_all() {
    let groups: Vec<i32> = LIVE.lock().map_or_else(|_| Vec::new(), |guard| guard.values().copied().collect());
    for pgid in groups {
        signal_group(pgid, libc::SIGKILL);
    }
}

pub fn signal_group(pgid: i32, signal: i32) {
    if pgid > 1 {
        unsafe { libc::killpg(pgid, signal) };
    }
}

#[cfg(test)]
fn group_alive(pgid: i32) -> bool {
    if pgid <= 1 {
        return false;
    }
    let result = unsafe { libc::killpg(pgid, 0) };
    return result == 0 || io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH);
}

fn register(pid: u32, group: i32) {
    if let Ok(mut guard) = LIVE.lock() {
        guard.insert(pid, group);
    }
}

fn deregister(pid: u32) {
    if let Ok(mut guard) = LIVE.lock() {
        guard.remove(&pid);
    }
}

fn pgid_of(pid: u32) -> i32 {
    return i32::try_from(pid).unwrap_or(0);
}

pub struct Process {
    program: OsString,
    args: Vec<OsString>,
    envs: Vec<(OsString, OsString)>,
}

impl Process {
    pub fn new(program: impl Into<OsString>) -> Self {
        return Self { program: program.into(), args: Vec::new(), envs: Vec::new() };
    }

    pub fn arg(&mut self, arg: impl Into<OsString>) -> &mut Self {
        self.args.push(arg.into());
        return self;
    }

    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        for arg in args {
            self.args.push(arg.into());
        }
        return self;
    }

    pub fn env(&mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> &mut Self {
        self.envs.push((key.into(), value.into()));
        return self;
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        for (key, value) in &self.envs {
            command.env(key, value);
        }
        command.process_group(0);
        command.kill_on_drop(true);
        command.stdin(Stdio::null());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        return command;
    }

    pub fn spawn_piped(&self) -> io::Result<Child> {
        let inner = self.command().spawn()?;
        return Ok(Child::adopt(inner));
    }

    pub async fn output(&self) -> io::Result<Output> {
        let inner = self.command().spawn()?;
        let pid = inner.id().unwrap_or(0);
        register(pid, pgid_of(pid));
        let result = inner.wait_with_output().await;
        deregister(pid);
        return result;
    }
}

pub struct Child {
    inner: tokio::process::Child,
    pid: u32,
    pgid: i32,
    alive: bool,
}

impl Child {
    fn adopt(inner: tokio::process::Child) -> Self {
        let pid = inner.id().unwrap_or(0);
        let group = pgid_of(pid);
        register(pid, group);
        return Self { inner, pid, pgid: group, alive: true };
    }

    #[cfg(test)]
    fn pgid(&self) -> i32 {
        return self.pgid;
    }

    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        return self.inner.stdout.take();
    }

    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        return self.inner.stderr.take();
    }

    pub fn terminate(&self) {
        signal_group(self.pgid, libc::SIGTERM);
    }

    pub fn kill(&self) {
        signal_group(self.pgid, libc::SIGKILL);
    }

    pub async fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = self.inner.wait().await?;
        self.finish();
        return Ok(status);
    }

    pub async fn cancel(&mut self, escalation: Duration) -> io::Result<ExitStatus> {
        self.terminate();
        if let Ok(result) = tokio::time::timeout(escalation, self.inner.wait()).await {
            let status = result?;
            self.finish();
            return Ok(status);
        }
        self.kill();
        let status = self.inner.wait().await?;
        self.finish();
        return Ok(status);
    }

    fn finish(&mut self) {
        if self.alive {
            self.alive = false;
            deregister(self.pid);
        }
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if self.alive {
            signal_group(self.pgid, libc::SIGKILL);
            deregister(self.pid);
            self.alive = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn stub(script: &str) -> Process {
        let mut process = Process::new("/bin/sh");
        process.arg("-c").arg(script);
        return process;
    }

    async fn wait_gone(pgid: i32) {
        for _ in 0..100 {
            if !group_alive(pgid) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn await_marker(path: &std::path::Path) {
        for _ in 0..500 {
            if path.exists() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("stub never installed its TERM trap");
    }

    #[tokio::test]
    async fn cancel_escalates_when_term_is_ignored() {
        let dir = tempfile::tempdir().expect("tempdir");
        let marker = dir.path().join("ready");
        let script = format!("trap \"\" TERM; : > '{}'; sleep 30", marker.display());
        let mut child = stub(&script).spawn_piped().expect("spawn");
        let pgid = child.pgid();
        assert!(pgid > 1);
        await_marker(&marker).await;
        let started = Instant::now();
        child.cancel(Duration::from_millis(300)).await.expect("cancel");
        let elapsed = started.elapsed();
        assert!(elapsed >= Duration::from_millis(250), "escalation delay was skipped: {elapsed:?}");
        assert!(elapsed < Duration::from_secs(5), "cancel took too long: {elapsed:?}");
        wait_gone(pgid).await;
        assert!(!group_alive(pgid), "process group survived the kill");
    }

    #[tokio::test]
    async fn cancel_is_prompt_when_term_is_honoured() {
        let mut child = stub("sleep 30").spawn_piped().expect("spawn");
        let pgid = child.pgid();
        let started = Instant::now();
        child.cancel(Duration::from_secs(5)).await.expect("cancel");
        assert!(started.elapsed() < Duration::from_secs(2), "did not exit on SIGTERM");
        wait_gone(pgid).await;
        assert!(!group_alive(pgid), "process group survived the kill");
    }

    #[tokio::test]
    async fn wait_reaps_without_killing_an_unrelated_group() {
        let mut child = stub("exit 0").spawn_piped().expect("spawn");
        let pgid = child.pgid();
        let status = child.wait().await.expect("wait");
        assert!(status.success());
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!group_alive(pgid));
    }

    #[tokio::test]
    async fn output_captures_stdout() {
        let mut process = Process::new("/bin/echo");
        process.arg("hello");
        let output = process.output().await.expect("output");
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
    }
}
