use std::process::ExitCode;

fn main() -> ExitCode {
    if let Err(error) = livewall::ui::run() {
        eprintln!("lwui: {error}");
        return ExitCode::FAILURE;
    }
    return ExitCode::SUCCESS;
}
