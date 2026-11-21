//! Runs the prototype on the machine's native backend (GTK on Linux, AppKit on macOS).

#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;
#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() -> impl std::process::Termination {
    env_logger::init();
    platform::run_app(
        platform::AppSource::Rust(platform::Root::of::<hayami_nsh::App>()),
        platform::AppOptions {
            title: "Hayami Review (Native Shell prototype)".to_owned(),
            #[cfg(target_os = "linux")]
            app_id: "dev.hayami.prototype.nsh".to_owned(),
            size: (1280, 800),
            ..Default::default()
        },
    )
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("The prototype runs on Linux and macOS only.");
