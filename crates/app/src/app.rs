use std::cell::RefCell;

use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;
use proto::{ErrorCode, Request, Response};
use serde::Serialize;
use tokio::sync::oneshot;

use crate::model::{Library, Settings};
use crate::paths::Paths;
use crate::store;

#[allow(clippy::large_enum_variant)]
pub enum Event {
    Request(Request, oneshot::Sender<Response>),
    Quit,
}

pub struct App {
    library: Library,
    settings: Settings,
    paused: bool,
    muted: bool,
}

impl App {
    pub fn new(paths: &Paths) -> (Self, Vec<store::Warning>) {
        let (library, library_warning) = store::load::<Library>(&paths.library_file());
        let (mut settings, settings_warning) = store::load::<Settings>(&paths.settings_file());
        settings.normalize();
        let warnings = library_warning.into_iter().chain(settings_warning).collect();
        let app = Self {
            muted: settings.muted,
            paused: false,
            library,
            settings,
        };
        return (app, warnings);
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Quit => quit(),
            Event::Request(request, reply) => {
                let response = self.dispatch(&request);
                let _ = reply.send(response);
            }
        }
    }

    fn dispatch(&mut self, request: &Request) -> Response {
        match request {
            Request::Status => {
                return ok(&StatusReport {
                    version: env!("CARGO_PKG_VERSION"),
                    paused: self.paused,
                    muted: self.muted,
                });
            }
            Request::Pause => {
                self.paused = true;
                return ok(&Self::state(self.paused, self.muted));
            }
            Request::Resume => {
                self.paused = false;
                return ok(&Self::state(self.paused, self.muted));
            }
            Request::TogglePause => {
                self.paused = !self.paused;
                return ok(&Self::state(self.paused, self.muted));
            }
            Request::Mute => {
                self.muted = true;
                return ok(&Self::state(self.paused, self.muted));
            }
            Request::Unmute => {
                self.muted = false;
                return ok(&Self::state(self.paused, self.muted));
            }
            Request::LibraryList => {
                return ok(&LibraryView { items: &self.library.items, folders: &self.library.folders });
            }
            Request::SettingsGet => return ok(&self.settings),
            Request::Quit => return Response::ok(serde_json::json!({})),
            _ => return Response::err(ErrorCode::NotImplemented, "command not implemented yet"),
        }
    }

    fn state(paused: bool, muted: bool) -> StateReport {
        return StateReport { paused, muted };
    }
}

#[derive(Serialize)]
struct StatusReport {
    version: &'static str,
    paused: bool,
    muted: bool,
}

#[derive(Serialize)]
struct StateReport {
    paused: bool,
    muted: bool,
}

#[derive(Serialize)]
struct LibraryView<'a> {
    items: &'a [crate::model::Item],
    folders: &'a [crate::model::Folder],
}

fn ok<T: Serialize>(value: &T) -> Response {
    match serde_json::to_value(value) {
        Ok(data) => Response::ok(data),
        Err(_) => Response::err(ErrorCode::Internal, "serialization failed"),
    }
}

fn quit() {
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        app.terminate(None);
    }
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

pub fn install(app: App) {
    APP.with(|cell| {
        *cell.borrow_mut() = Some(app);
    });
}

pub fn post(event: Event) {
    dispatch2::DispatchQueue::main().exec_async(move || {
        APP.with(|cell| {
            if let Some(app) = cell.borrow_mut().as_mut() {
                app.handle(event);
            }
        });
    });
}
