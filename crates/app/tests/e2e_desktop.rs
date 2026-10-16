mod support;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use livewall::proto::{ApplyTarget, Request, ResponseBody};

use support::{Guard, POLL, RESTORE_TIMEOUT, Sandbox, TAKEOVER_TIMEOUT, await_pictures, current_pictures};

fn main() -> ExitCode {
    if std::env::var("LW_E2E").as_deref() != Ok("1") {
        println!("e2e_desktop: skipped (set LW_E2E=1 to run)");
        return ExitCode::SUCCESS;
    }
    let Some(fixture) = fixture() else {
        println!("e2e_desktop: skipped (run scripts/fixtures.sh; no fixture video found)");
        return ExitCode::SUCCESS;
    };

    let guard = Guard::capture();
    let result = std::panic::catch_unwind(|| {
        clean_takeover_and_restore(&fixture);
        crash_recovery(&fixture);
    });
    drop(guard);

    return match result {
        Ok(()) => {
            println!("e2e_desktop: ok");
            ExitCode::SUCCESS
        }
        Err(_) => ExitCode::FAILURE,
    };
}

fn fixture() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
    for name in ["clip-h264-720p60.mp4", "clip-hevc-1080p30.mp4", "portrait.mp4"] {
        let candidate = root.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    return None;
}

fn clean_takeover_and_restore(fixture: &Path) {
    println!("e2e_desktop: clean takeover/restore");
    let sandbox = Sandbox::new("clean");
    let original = current_pictures();
    sandbox.launch();
    let response = sandbox.add_and_apply(fixture);
    assert!(response, "library add/apply failed");
    let still = sandbox.await_owned();
    assert!(still.contains("Stills"), "desktop still was not applied: {still}");

    sandbox.quit();
    sandbox.await_idle();
    await_pictures(&original, RESTORE_TIMEOUT);
    assert_eq!(current_pictures(), original, "desktop picture was not restored exactly");
}

fn crash_recovery(fixture: &Path) {
    println!("e2e_desktop: crash recovery");
    let sandbox = Sandbox::new("crash");
    let original = current_pictures();
    sandbox.launch();
    assert!(sandbox.add_and_apply(fixture), "library add/apply failed");
    let still = sandbox.await_owned();
    assert!(still.contains("Stills"), "desktop still was not applied: {still}");
    assert!(sandbox.swap_state_exists(), "swap state was not persisted");

    sandbox.kill9();

    sandbox.launch();
    let retaken = sandbox.await_owned();
    assert!(retaken.contains("Stills"), "desktop still was not retaken after crash: {retaken}");
    sandbox.quit();
    sandbox.await_idle();
    await_pictures(&original, RESTORE_TIMEOUT);
    assert_eq!(current_pictures(), original, "desktop picture was not restored after crash");
    assert!(!sandbox.swap_state_exists(), "swap state was not cleared");
}

impl Sandbox {
    fn swap_state_exists(&self) -> bool {
        return self.data_dir().join("swap-state.json").exists();
    }

    fn add_and_apply(&self, fixture: &Path) -> bool {
        let added =
            self.request(Request::LibraryAdd { paths: vec![fixture.to_string_lossy().into_owned()], folder: None });
        let ResponseBody::Ok { data } = added.body else { return false };
        if data["added"].as_u64().unwrap_or(0) != 1 {
            return false;
        }
        let list = self.request(Request::LibraryList);
        let ResponseBody::Ok { data } = list.body else { return false };
        let Some(items) = data["items"].as_array() else { return false };
        let Some(id) = items.first().and_then(|item| item["id"].as_str()) else { return false };
        let applied =
            self.request(Request::Apply { item: Some(id.to_string()), target: ApplyTarget::Default, transition: None });
        return matches!(applied.body, ResponseBody::Ok { .. });
    }

    fn await_owned(&self) -> String {
        let deadline = Instant::now() + TAKEOVER_TIMEOUT;
        loop {
            if let Some(current) = self.owned_current() {
                return current;
            }
            assert!(Instant::now() < deadline, "desktop takeover did not complete");
            std::thread::sleep(POLL);
        }
    }

    fn await_idle(&self) {
        let deadline = Instant::now() + RESTORE_TIMEOUT;
        while Instant::now() < deadline {
            if self.request_opt(Request::Status).is_none() {
                return;
            }
            std::thread::sleep(POLL);
        }
    }

    fn owned_current(&self) -> Option<String> {
        let response = self.request_opt(Request::Desktop)?;
        let ResponseBody::Ok { data } = response.body else { return None };
        let surfaces = data["surfaces"].as_array()?;
        if surfaces.is_empty() {
            return None;
        }
        let all_owned = surfaces.iter().all(|surface| surface["state"].as_str() == Some("owned"));
        if !all_owned {
            return None;
        }
        for surface in surfaces {
            let current = surface["current"].as_str().unwrap_or_default();
            if !current.contains("Stills") {
                return None;
            }
        }
        let current = surfaces.iter().find_map(|surface| surface["current"].as_str().map(ToString::to_string))?;
        return Some(current);
    }
}
