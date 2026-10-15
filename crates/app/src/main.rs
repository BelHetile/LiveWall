use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::OnceLock;
use std::time::Duration;

use livewall::{App, BindOutcome, Event, Paths, cleanup, install, post, prepare, proto, serve};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol};
use tracing_subscriber::layer::SubscriberExt;

static SOCKET_PATH: OnceLock<PathBuf> = OnceLock::new();

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _notification: &NSNotification) {
            if let Some(path) = SOCKET_PATH.get() {
                cleanup(path);
            }
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        return unsafe { msg_send![super(this), init] };
    }
}

fn main() -> ExitCode {
    init_logging();
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("LiveWall must run on the main thread");
        return ExitCode::FAILURE;
    };

    let paths = Paths::detect();
    if let Err(error) = paths.ensure() {
        eprintln!("LiveWall: cannot create data directory: {error}");
        return ExitCode::FAILURE;
    }

    let socket_path = proto::socket_path();
    let _ = SOCKET_PATH.set(socket_path.clone());

    let listener = match prepare(&socket_path) {
        Ok(BindOutcome::Bound(listener)) => listener,
        Ok(BindOutcome::AlreadyRunning) => {
            println!("already running");
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("LiveWall: cannot bind {}: {error}", socket_path.display());
            return ExitCode::FAILURE;
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("LiveWall: cannot start tokio runtime: {error}");
            return ExitCode::FAILURE;
        }
    };

    let adopted = {
        let _guard = runtime.enter();
        tokio::net::UnixListener::from_std(listener)
    };
    let listener = match adopted {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("LiveWall: cannot adopt socket: {error}");
            return ExitCode::FAILURE;
        }
    };

    {
        let _guard = runtime.enter();
        let (app, warnings) = App::new(&paths, mtm);
        install(app);
        for warning in warnings {
            tracing::warn!(%warning, "store");
        }

        runtime.spawn(serve(listener));
        runtime.spawn(watch_signals());

        let application = NSApplication::sharedApplication(mtm);
        let delegate = Delegate::new(mtm);
        application.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        let _ = application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        application.run();
    }

    cleanup(&socket_path);
    runtime.shutdown_timeout(Duration::from_secs(2));
    return ExitCode::SUCCESS;
}

async fn watch_signals() {
    let Ok(mut terminate) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) else {
        return;
    };
    let Ok(mut interrupt) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()) else {
        return;
    };
    tokio::select! {
        _ = terminate.recv() => {}
        _ = interrupt.recv() => {}
    }
    post(Event::Quit);
}

fn init_logging() {
    let oslog = tracing_oslog::OsLogger::new("io.github.belhetile.livewall", "engine");
    let subscriber = tracing_subscriber::registry().with(oslog);
    let _ = tracing::subscriber::set_global_default(subscriber);
}
