//! `eloane-session`: started by autologin on tty1. Detects the render GPU,
//! launches Steam inside gamescope (or cage without a usable GPU) and restarts
//! it on crash. Exits 0 when the user quits, non-zero after too many crashes
//! so that getty takes over.

use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use anyhow::Context;
use clap::Parser;

use eloane_session::config::{CompositorChoice, Config};
use eloane_session::launch::{
    build_launch, fallback_compositor, resolve_compositor, shell_quote, Compositor, CrashTracker,
    Launch,
};

#[derive(Debug, Parser)]
#[command(
    name = "eloane-session",
    version,
    about = "Eloane gaming session (gamescope + Steam)"
)]
struct Cli {
    /// Configuration file (default: /etc/eloane/session.toml, optional).
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Print the command and environment without executing anything.
    #[arg(long)]
    dry_run: bool,

    /// sysfs root used for GPU detection (testing).
    #[arg(long, value_name = "PATH", default_value = eloane_hw::DEFAULT_SYSFS_ROOT, hide = true)]
    sysfs_root: PathBuf,
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => code,
        Err(e) => {
            log::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> anyhow::Result<ExitCode> {
    let config = Config::load(cli.config.as_deref())?;

    let gpus = eloane_hw::detect_gpus_at(&cli.sysfs_root).unwrap_or_else(|e| {
        log::warn!("GPU detection failed: {e}");
        Vec::new()
    });
    for gpu in &gpus {
        log::info!(
            "found {} {} ({}) [{}] driver={}",
            gpu.kind,
            gpu.vendor,
            gpu.vk_device_id(),
            gpu.card,
            gpu.driver.as_deref().unwrap_or("none"),
        );
    }
    let plan = eloane_hw::select_render_gpu(&gpus, config.prefer_gpu.as_ref());
    match &plan {
        Some(p) => log::info!(
            "render GPU: {} {} ({}){}{}",
            p.gpu.vendor,
            p.gpu.kind,
            p.prefer_vk_device,
            if p.hybrid { ", hybrid" } else { "" },
            if p.overridden { ", from config" } else { "" },
        ),
        None => log::warn!("no GPU detected, letting gamescope choose"),
    }

    let compositor = resolve_compositor(config.compositor, &gpus);
    log::info!(
        "compositor: {compositor}{}",
        match config.compositor {
            CompositorChoice::Auto if compositor == Compositor::Cage =>
                " (auto: no non-virtual render GPU, software rendering)",
            CompositorChoice::Auto => " (auto)",
            _ => " (from config)",
        }
    );

    if cli.dry_run {
        print_dry_run(&build_launch(&config, compositor, plan.as_ref()));
        if let Some(next) = fallback_compositor(config.compositor, compositor) {
            println!(
                "# fallback after {} crashes within {}s",
                config.max_restarts + 1,
                config.restart_window_secs
            );
            print_dry_run(&build_launch(&config, next, plan.as_ref()));
        }
        return Ok(ExitCode::SUCCESS);
    }
    supervise(&config, compositor, plan.as_ref())
}

fn print_dry_run(launch: &Launch) {
    println!("# environment");
    if launch.env.is_empty() {
        println!("# (none)");
    }
    for (k, v) in &launch.env {
        println!("{k}={}", shell_quote(v));
    }
    println!("# command");
    println!(
        "{}",
        Launch {
            env: Vec::new(),
            ..launch.clone()
        }
    );
}

/// Runs the session, restarting it on crash. In `auto` mode, falls back once
/// from gamescope to cage when gamescope exceeds its crash budget.
fn supervise(
    config: &Config,
    mut compositor: Compositor,
    plan: Option<&eloane_hw::RenderPlan>,
) -> anyhow::Result<ExitCode> {
    let window = Duration::from_secs(config.restart_window_secs);
    let mut tracker = CrashTracker::new(config.max_restarts, window);
    let mut launch = build_launch(config, compositor, plan);
    loop {
        log::info!("starting: {launch}");
        let status = Command::new(&launch.program)
            .args(&launch.args)
            .envs(launch.env.iter().map(|(k, v)| (k, v)))
            .status()
            .with_context(|| format!("failed to start {}", launch.program))?;

        if status.success() {
            log::info!("session exited normally");
            return Ok(ExitCode::SUCCESS);
        }
        log::warn!("{compositor} session crashed ({status})");
        if !tracker.record_crash(Instant::now()) {
            match fallback_compositor(config.compositor, compositor) {
                Some(next) => {
                    log::warn!(
                        "{compositor} crashed {} times within {}s, falling back to {next} (software rendering); set `compositor` in session.toml to override",
                        tracker.recent(),
                        config.restart_window_secs,
                    );
                    compositor = next;
                    launch = build_launch(config, compositor, plan);
                    tracker = CrashTracker::new(config.max_restarts, window);
                    continue;
                }
                None => {
                    log::error!(
                        "{} crashes within {}s (max_restarts = {}), giving up",
                        tracker.recent(),
                        config.restart_window_secs,
                        config.max_restarts
                    );
                    return Ok(ExitCode::FAILURE);
                }
            }
        }
        log::info!(
            "restarting session ({}/{} in window)",
            tracker.recent(),
            config.max_restarts
        );
        std::thread::sleep(Duration::from_secs(1));
    }
}
