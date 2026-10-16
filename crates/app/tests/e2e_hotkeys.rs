mod support;

use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use livewall::proto::{ErrorCode, HotkeyAction, Request, Response, ResponseBody};
use livewall::{App, BindOutcome, Paths, install, prepare, serve, synthesize_press};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::{NSDate, NSRunLoop};

use support::Sandbox;

const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(20);
const PUMP: Duration = Duration::from_millis(50);

fn main() -> ExitCode {
    if std::env::var("LW_E2E").as_deref() != Ok("1") {
        println!("e2e_hotkeys: skipped (set LW_E2E=1 to run)");
        return ExitCode::SUCCESS;
    }
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("e2e_hotkeys: must run on the main thread");
        return ExitCode::FAILURE;
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        in_process(mtm);
        cross_process();
    }));
    return match result {
        Ok(()) => {
            println!("e2e_hotkeys: ok");
            ExitCode::SUCCESS
        }
        Err(_) => ExitCode::FAILURE,
    };
}

fn in_process(mtm: MainThreadMarker) {
    println!("e2e_hotkeys: in-process dispatch, validation and i18n");
    let dir = tempfile::Builder::new().prefix("livewall-hotkeys-").tempdir().expect("tempdir");
    let data = dir.path().join("data");
    let socket = dir.path().join("control.sock");
    unsafe {
        std::env::set_var("LIVEWALL_DATA_DIR", &data);
        std::env::set_var("LIVEWALL_SOCKET", &socket);
    }
    let paths = Paths::detect();
    paths.ensure().expect("data dir");
    let listener = match prepare(&socket).expect("prepare") {
        BindOutcome::Bound(listener) => listener,
        BindOutcome::AlreadyRunning => panic!("socket already in use"),
    };
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("tokio");
    let listener = {
        let _guard = runtime.enter();
        tokio::net::UnixListener::from_std(listener).expect("adopt socket")
    };
    {
        let _guard = runtime.enter();
        let (app, _warnings) = App::new(&paths, mtm);
        install(app);
        runtime.spawn(serve(listener));
    }
    let application = NSApplication::sharedApplication(mtm);
    let _ = application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let report = ok(&socket, Request::HotkeysGet);
    let entries = report.as_array().expect("hotkeys array");
    assert_eq!(entries.len(), HotkeyAction::ALL.len(), "every action reported: {report}");
    for action in HotkeyAction::ALL {
        let entry = entry_for(&report, action);
        assert!(entry["combo"].is_object(), "{action:?} has a default combo");
        assert!(entry["display"].as_str().is_some(), "{action:?} has a display string");
    }

    let reserved =
        exchange(&socket, Request::HotkeysSet { action: HotkeyAction::TogglePause, combo: Some("cmd+space".into()) });
    let body = expect_error(reserved, ErrorCode::Invalid);
    assert!(body.contains("Spotlight"), "reserved reason was {body:?}");

    let invalid_key = exchange(
        &socket,
        Request::HotkeysSet { action: HotkeyAction::ToggleMute, combo: Some("ctrl+opt+cmd+banana".into()) },
    );
    let body = expect_error(invalid_key, ErrorCode::Invalid);
    assert!(body.contains("banana"), "unknown key reason was {body:?}");

    let missing_modifier =
        exchange(&socket, Request::HotkeysSet { action: HotkeyAction::ToggleMute, combo: Some("shift+j".into()) });
    expect_error(missing_modifier, ErrorCode::Invalid);

    let bound = exchange(
        &socket,
        Request::HotkeysSet { action: HotkeyAction::TogglePause, combo: Some("ctrl+opt+cmd+f18".into()) },
    );
    assert!(matches!(bound.body, ResponseBody::Ok { .. }), "{bound:?}");
    let report = ok(&socket, Request::HotkeysGet);
    let entry = entry_for(&report, HotkeyAction::TogglePause);
    assert_eq!(entry["display"].as_str(), Some("⌃⌥⌘F18"));
    assert_eq!(entry["registered"].as_bool(), Some(true));

    let duplicate =
        exchange(&socket, Request::HotkeysSet { action: HotkeyAction::Next, combo: Some("ctrl+opt+cmd+f18".into()) });
    let body = expect_error(duplicate, ErrorCode::Invalid);
    assert!(body.contains("another action"), "duplicate reason was {body:?}");

    assert!(!status(&socket)["paused"].as_bool().unwrap_or(true));
    assert!(synthesize_press(HotkeyAction::TogglePause), "synthetic hotkey event was not delivered");
    assert_eq!(status(&socket)["paused"].as_bool(), Some(true));
    assert!(synthesize_press(HotkeyAction::TogglePause));
    assert_eq!(status(&socket)["paused"].as_bool(), Some(false));

    let mut patch = serde_json::Map::new();
    patch.insert("language".into(), serde_json::Value::String("ru".into()));
    let response = exchange(&socket, Request::SettingsSet { patch: serde_json::Value::Object(patch) });
    assert!(matches!(response.body, ResponseBody::Ok { .. }), "{response:?}");
    let menu = ok(&socket, Request::Menu);
    let titles: Vec<String> = menu["items"]
        .as_array()
        .expect("menu items")
        .iter()
        .filter_map(|item| item["title"].as_str().map(ToString::to_string))
        .collect();
    assert!(titles.iter().any(|title| title == "Пауза"), "menu was not localized: {titles:?}");
    assert!(titles.iter().any(|title| title == "Завершить LiveWall"), "menu was not localized: {titles:?}");

    let login = ok(&socket, Request::LoginItemGet);
    assert!(login["status"].as_str().is_some(), "login item status: {login}");

    for action in HotkeyAction::ALL {
        let cleared = exchange(&socket, Request::HotkeysSet { action, combo: None });
        assert!(matches!(cleared.body, ResponseBody::Ok { .. }), "{cleared:?}");
    }
    let report = ok(&socket, Request::HotkeysGet);
    for action in HotkeyAction::ALL {
        assert_eq!(entry_for(&report, action)["registered"].as_bool(), Some(false), "{action:?} still registered");
    }
}

fn cross_process() {
    println!("e2e_hotkeys: cross-process conflicts");
    let first = Sandbox::new("conflict-a");
    first.launch();
    let report = first.ok(Request::HotkeysGet);
    for action in HotkeyAction::ALL {
        let entry = entry_for(&report, action);
        assert_eq!(entry["registered"].as_bool(), Some(true), "first app did not register {action:?}: {entry}");
        assert!(entry["conflict"].is_null(), "unexpected conflict in the first app: {entry}");
    }

    let second = Sandbox::new("conflict-b");
    second.launch();
    let report = second.ok(Request::HotkeysGet);
    for action in HotkeyAction::ALL {
        let entry = entry_for(&report, action);
        assert_eq!(entry["registered"].as_bool(), Some(false), "second app stole {action:?}: {entry}");
        let conflict = entry["conflict"].as_str().unwrap_or_default();
        assert!(conflict.contains("another app"), "second app conflict reason was {conflict:?}");
    }

    first.quit();
    second.quit();

    let third = Sandbox::new("conflict-c");
    third.launch();
    let report = third.ok(Request::HotkeysGet);
    for action in HotkeyAction::ALL {
        let entry = entry_for(&report, action);
        assert_eq!(entry["registered"].as_bool(), Some(true), "hotkeys were not released after quit: {entry}");
    }
    third.quit();
}

fn entry_for(report: &serde_json::Value, action: HotkeyAction) -> serde_json::Value {
    let name = serde_json::to_value(action).expect("action name");
    return report
        .as_array()
        .and_then(|entries| entries.iter().find(|entry| entry["action"] == name))
        .cloned()
        .unwrap_or_else(|| panic!("no report entry for {action:?}: {report}"));
}

fn ok(socket: &Path, request: Request) -> serde_json::Value {
    let response = exchange(socket, request);
    return match response.body {
        ResponseBody::Ok { data } => data,
        ResponseBody::Err(body) => panic!("unexpected error {body:?}"),
    };
}

fn expect_error(response: Response, code: ErrorCode) -> String {
    return match response.body {
        ResponseBody::Err(body) => {
            assert_eq!(body.code, code, "{body:?}");
            body.message
        }
        ResponseBody::Ok { .. } => panic!("expected an error, got {response:?}"),
    };
}

fn status(socket: &Path) -> serde_json::Value {
    return ok(socket, Request::Status);
}

fn exchange(socket: &Path, request: Request) -> Response {
    let socket = socket.to_path_buf();
    let worker = std::thread::spawn(move || support::send(&socket, request, Duration::from_secs(10)));
    let deadline = Instant::now() + EXCHANGE_TIMEOUT;
    loop {
        if worker.is_finished() {
            return worker.join().expect("join exchange").expect("exchange");
        }
        assert!(Instant::now() < deadline, "no response from the in-process app");
        pump(PUMP.as_secs_f64());
    }
}

fn pump(seconds: f64) {
    let limit = NSDate::dateWithTimeIntervalSinceNow(seconds);
    NSRunLoop::mainRunLoop().runUntilDate(&limit);
}
