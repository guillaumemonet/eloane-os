//! Pure construction of the session command line and crash accounting.

use std::collections::VecDeque;
use std::fmt;
use std::time::{Duration, Instant};

use eloane_hw::{Gpu, GpuKind, RenderPlan};
use serde::Serialize;

use crate::config::{CompositorChoice, Config};

/// gamescope binary.
pub const GAMESCOPE: &str = "gamescope";
/// cage binary (wlroots kiosk compositor, software rendering fallback).
pub const CAGE: &str = "cage";

/// Environment forcing wlroots software rendering in cage.
pub const CAGE_ENV: &[(&str, &str)] =
    &[("WLR_RENDERER", "pixman"), ("WLR_NO_HARDWARE_CURSORS", "1")];

/// Effective compositor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Compositor {
    Gamescope,
    Cage,
}

impl Compositor {
    pub fn name(self) -> &'static str {
        match self {
            Compositor::Gamescope => GAMESCOPE,
            Compositor::Cage => CAGE,
        }
    }
}

impl fmt::Display for Compositor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Whether at least one GPU can plausibly run gamescope (Vulkan): anything
/// but an emulated adapter.
pub fn has_render_gpu(gpus: &[Gpu]) -> bool {
    gpus.iter().any(|g| g.kind != GpuKind::Virtual)
}

/// Resolves the configured choice: `auto` picks gamescope when a non-virtual
/// GPU exists, cage otherwise (no GPU at all, or only virtio/QXL/...).
pub fn resolve_compositor(choice: CompositorChoice, gpus: &[Gpu]) -> Compositor {
    match choice {
        CompositorChoice::Gamescope => Compositor::Gamescope,
        CompositorChoice::Cage => Compositor::Cage,
        CompositorChoice::Auto if has_render_gpu(gpus) => Compositor::Gamescope,
        CompositorChoice::Auto => Compositor::Cage,
    }
}

/// Compositor to switch to once `current` exceeded its crash budget, if any.
/// Only `auto` falls back, once, from gamescope to cage.
pub fn fallback_compositor(choice: CompositorChoice, current: Compositor) -> Option<Compositor> {
    match (choice, current) {
        (CompositorChoice::Auto, Compositor::Gamescope) => Some(Compositor::Cage),
        _ => None,
    }
}

/// A fully resolved command to execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    /// Variables added to the inherited environment.
    pub env: Vec<(String, String)>,
}

/// Builds the session command.
///
/// - gamescope: `gamescope -e -f [--prefer-vk-device ..] [-W w] [-H h] [extra] -- <command>`
///   plus the PRIME offload environment of the render plan;
/// - cage: `cage -s -- <command>` with `WLR_RENDERER=pixman WLR_NO_HARDWARE_CURSORS=1`
///   (gamescope options and GPU selection are ignored; `-s` keeps VT switching
///   available so a text console can be reached with Ctrl+Alt+F2).
pub fn build_launch(config: &Config, compositor: Compositor, plan: Option<&RenderPlan>) -> Launch {
    match compositor {
        Compositor::Gamescope => {
            let mut args: Vec<String> = vec!["-e".into(), "-f".into()];
            if let Some(plan) = plan {
                args.extend(plan.gamescope_args());
            }
            if let Some(w) = config.width {
                args.extend(["-W".to_string(), w.to_string()]);
            }
            if let Some(h) = config.height {
                args.extend(["-H".to_string(), h.to_string()]);
            }
            args.extend(config.gamescope_args.iter().cloned());
            args.push("--".into());
            args.extend(config.command.iter().cloned());
            Launch {
                program: GAMESCOPE.to_string(),
                args,
                env: plan.map(|p| p.env.clone()).unwrap_or_default(),
            }
        }
        Compositor::Cage => {
            let mut args = vec!["-s".to_string(), "--".to_string()];
            args.extend(config.command.iter().cloned());
            Launch {
                program: CAGE.to_string(),
                args,
                env: CAGE_ENV
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            }
        }
    }
}

impl fmt::Display for Launch {
    /// Shell-like rendering: `VAR=value ... program args...`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (k, v) in &self.env {
            write!(f, "{k}={} ", shell_quote(v))?;
        }
        f.write_str(&shell_quote(&self.program))?;
        for a in &self.args {
            write!(f, " {}", shell_quote(a))?;
        }
        Ok(())
    }
}

/// Quotes a word for POSIX shells when needed.
pub fn shell_quote(s: &str) -> String {
    let safe = !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./:=,+@%".contains(&b));
    if safe {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// Counts crashes in a sliding time window.
#[derive(Debug)]
pub struct CrashTracker {
    max_restarts: u32,
    window: Duration,
    crashes: VecDeque<Instant>,
}

impl CrashTracker {
    pub fn new(max_restarts: u32, window: Duration) -> Self {
        CrashTracker {
            max_restarts,
            window,
            crashes: VecDeque::new(),
        }
    }

    /// Records a crash at `now`; returns `true` if a restart is still allowed,
    /// `false` once more than `max_restarts` crashes happened within the window.
    pub fn record_crash(&mut self, now: Instant) -> bool {
        while let Some(&first) = self.crashes.front() {
            if now.duration_since(first) > self.window {
                self.crashes.pop_front();
            } else {
                break;
            }
        }
        self.crashes.push_back(now);
        self.crashes.len() <= self.max_restarts as usize
    }

    /// Number of crashes currently inside the window.
    pub fn recent(&self) -> usize {
        self.crashes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eloane_hw::Vendor;

    fn nvidia_plan() -> RenderPlan {
        RenderPlan {
            gpu: Gpu {
                card: "card0".into(),
                pci_address: Some("0000:01:00.0".into()),
                vendor: Vendor::Nvidia,
                vendor_id: 0x10de,
                device_id: 0x28b8,
                driver: Some("nvidia".into()),
                boot_vga: false,
                kind: GpuKind::Discrete,
            },
            hybrid: true,
            overridden: false,
            prefer_vk_device: "10de:28b8".into(),
            env: eloane_hw::prime_env(Vendor::Nvidia),
        }
    }

    #[test]
    fn default_command_without_gpu() {
        let l = build_launch(&Config::default(), Compositor::Gamescope, None);
        assert_eq!(l.program, "gamescope");
        assert_eq!(
            l.args,
            [
                "-e",
                "-f",
                "--",
                "steam",
                "-gamepadui",
                "-steamos3",
                "-steampal",
                "-steamdeck"
            ]
        );
        assert!(l.env.is_empty());
    }

    #[test]
    fn full_command_with_hybrid_plan() {
        let config = Config {
            width: Some(1920),
            height: Some(1080),
            gamescope_args: vec!["--adaptive-sync".into()],
            command: vec!["steam".into(), "-gamepadui".into()],
            ..Config::default()
        };
        let l = build_launch(&config, Compositor::Gamescope, Some(&nvidia_plan()));
        assert_eq!(
            l.args,
            [
                "-e",
                "-f",
                "--prefer-vk-device",
                "10de:28b8",
                "-W",
                "1920",
                "-H",
                "1080",
                "--adaptive-sync",
                "--",
                "steam",
                "-gamepadui"
            ]
        );
        assert_eq!(l.env.len(), 3);
        assert_eq!(
            l.to_string(),
            "__NV_PRIME_RENDER_OFFLOAD=1 __GLX_VENDOR_LIBRARY_NAME=nvidia \
             __VK_LAYER_NV_optimus=NVIDIA_only gamescope -e -f --prefer-vk-device 10de:28b8 \
             -W 1920 -H 1080 --adaptive-sync -- steam -gamepadui"
        );
    }

    #[test]
    fn quoting() {
        assert_eq!(shell_quote("abc"), "abc");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn crash_window() {
        let t0 = Instant::now();
        let mut t = CrashTracker::new(3, Duration::from_secs(60));
        assert!(t.record_crash(t0));
        assert!(t.record_crash(t0 + Duration::from_secs(1)));
        assert!(t.record_crash(t0 + Duration::from_secs(2)));
        // Fourth crash within 60 s: give up.
        assert!(!t.record_crash(t0 + Duration::from_secs(3)));

        let mut t = CrashTracker::new(3, Duration::from_secs(60));
        assert!(t.record_crash(t0));
        assert!(t.record_crash(t0 + Duration::from_secs(10)));
        assert!(t.record_crash(t0 + Duration::from_secs(20)));
        // The first crash has left the window.
        assert!(t.record_crash(t0 + Duration::from_secs(61)));
        assert_eq!(t.recent(), 3);
    }

    #[test]
    fn zero_restarts_gives_up_immediately() {
        let mut t = CrashTracker::new(0, Duration::from_secs(60));
        assert!(!t.record_crash(Instant::now()));
    }

    fn gpu(vendor_id: u16, device_id: u16, kind: GpuKind) -> Gpu {
        Gpu {
            card: "card0".into(),
            pci_address: None,
            vendor: Vendor::from_pci_id(vendor_id),
            vendor_id,
            device_id,
            driver: None,
            boot_vga: false,
            kind,
        }
    }

    #[test]
    fn auto_uses_cage_without_render_gpu() {
        let virtio = [gpu(0x1af4, 0x1050, GpuKind::Virtual)];
        assert_eq!(
            resolve_compositor(CompositorChoice::Auto, &virtio),
            Compositor::Cage
        );
        assert_eq!(
            resolve_compositor(CompositorChoice::Auto, &[]),
            Compositor::Cage
        );
        // Explicit choice wins.
        assert_eq!(
            resolve_compositor(CompositorChoice::Gamescope, &virtio),
            Compositor::Gamescope
        );
    }

    #[test]
    fn auto_uses_gamescope_on_hybrid() {
        let hybrid = [
            gpu(0x10de, 0x28b8, GpuKind::Discrete),
            gpu(0x8086, 0xa7a0, GpuKind::Integrated),
        ];
        assert_eq!(
            resolve_compositor(CompositorChoice::Auto, &hybrid),
            Compositor::Gamescope
        );
        assert_eq!(
            resolve_compositor(CompositorChoice::Cage, &hybrid),
            Compositor::Cage
        );
    }

    #[test]
    fn cage_command_ignores_gamescope_options() {
        let config = Config {
            width: Some(1920),
            height: Some(1080),
            gamescope_args: vec!["--adaptive-sync".into()],
            ..Config::default()
        };
        let l = build_launch(&config, Compositor::Cage, Some(&nvidia_plan()));
        assert_eq!(
            l.to_string(),
            "WLR_RENDERER=pixman WLR_NO_HARDWARE_CURSORS=1 cage -s -- steam -gamepadui -steamos3 -steampal -steamdeck"
        );
    }

    #[test]
    fn fallback_only_once_and_only_in_auto() {
        assert_eq!(
            fallback_compositor(CompositorChoice::Auto, Compositor::Gamescope),
            Some(Compositor::Cage)
        );
        assert_eq!(
            fallback_compositor(CompositorChoice::Auto, Compositor::Cage),
            None
        );
        assert_eq!(
            fallback_compositor(CompositorChoice::Gamescope, Compositor::Gamescope),
            None
        );
        assert_eq!(
            fallback_compositor(CompositorChoice::Cage, Compositor::Cage),
            None
        );
    }
}
