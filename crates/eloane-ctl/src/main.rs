//! `eloane-ctl`: inspection CLI for Eloane.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::{Parser, Subcommand};
use eloane_hw::{Gpu, RenderPlan};
use eloane_session::config::Config;
use eloane_session::launch::{resolve_compositor, Compositor};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "eloane-ctl", version, about = "Eloane control and diagnostics")]
struct Cli {
    /// Session configuration file (default: /etc/eloane/session.toml, optional).
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    /// sysfs root used for GPU detection (testing).
    #[arg(long, global = true, value_name = "PATH", default_value = eloane_hw::DEFAULT_SYSFS_ROOT, hide = true)]
    sysfs_root: PathBuf,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// List detected GPUs and the preferred render GPU.
    Gpu {
        /// Machine readable output.
        #[arg(long)]
        json: bool,
    },
    /// Print `export VAR=...` lines for the preferred GPU (PRIME offload).
    Env,
    /// Show Eloane version, kernel and preferred GPU.
    Info,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("eloane-ctl: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> anyhow::Result<()> {
    let config = Config::load(cli.config.as_deref())?;
    let gpus = eloane_hw::detect_gpus_at(&cli.sysfs_root).context("GPU detection failed")?;
    let plan = eloane_hw::select_render_gpu(&gpus, config.prefer_gpu.as_ref());
    let compositor = resolve_compositor(config.compositor, &gpus);

    match cli.command {
        Cmd::Gpu { json: true } => {
            #[derive(Serialize)]
            struct Out<'a> {
                gpus: &'a [Gpu],
                hybrid: bool,
                preferred: Option<&'a RenderPlan>,
                compositor: Compositor,
            }
            let out = Out {
                gpus: &gpus,
                hybrid: eloane_hw::is_hybrid(&gpus),
                preferred: plan.as_ref(),
                compositor,
            };
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        Cmd::Gpu { json: false } => print!("{}", gpu_table(&gpus, plan.as_ref())),
        Cmd::Env => match &plan {
            Some(p) if compositor == Compositor::Gamescope && !p.env.is_empty() => {
                for (k, v) in &p.env {
                    println!("export {k}={v}");
                }
            }
            _ => eprintln!("# no PRIME offload variables needed"),
        },
        Cmd::Info => {
            println!("Eloane   {}", env!("CARGO_PKG_VERSION"));
            println!("Kernel   {}", kernel_release());
            match &plan {
                Some(p) => println!(
                    "GPU      {} {} ({}, {}{})",
                    p.gpu.vendor,
                    p.prefer_vk_device,
                    p.gpu.kind,
                    p.gpu.driver.as_deref().unwrap_or("no driver"),
                    if p.hybrid {
                        ", hybrid PRIME offload"
                    } else {
                        ""
                    },
                ),
                None => println!("GPU      none detected"),
            }
            println!(
                "Session  {compositor} (compositor = {})",
                config.compositor.name()
            );
        }
    }
    Ok(())
}

fn kernel_release() -> String {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Renders a fixed-width table of GPUs.
fn gpu_table(gpus: &[Gpu], plan: Option<&RenderPlan>) -> String {
    if gpus.is_empty() {
        return "no GPU detected\n".to_string();
    }
    let header = ["", "CARD", "PCI", "VENDOR", "ID", "DRIVER", "KIND", "BOOT"];
    let rows: Vec<[String; 8]> = gpus
        .iter()
        .map(|g| {
            let preferred = plan.is_some_and(|p| p.gpu.card == g.card);
            [
                if preferred { "*" } else { "" }.to_string(),
                g.card.clone(),
                g.pci_address.clone().unwrap_or_else(|| "-".into()),
                g.vendor.to_string(),
                g.vk_device_id(),
                g.driver.clone().unwrap_or_else(|| "-".into()),
                g.kind.to_string(),
                if g.boot_vga { "yes" } else { "no" }.to_string(),
            ]
        })
        .collect();

    let mut widths = header.map(str::len);
    for row in &rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.len());
        }
    }
    let mut out = String::new();
    let mut push_row = |cells: &[&str]| {
        let line: Vec<String> = cells
            .iter()
            .zip(widths)
            .map(|(c, w)| format!("{c:<w$}"))
            .collect();
        out.push_str(line.join("  ").trim_end());
        out.push('\n');
    };
    push_row(&header);
    for row in &rows {
        push_row(&row.each_ref().map(String::as_str));
    }
    match plan {
        Some(p) => out.push_str(&format!(
            "\npreferred: {} ({} {}){}{}\n",
            p.gpu.card,
            p.gpu.vendor,
            p.prefer_vk_device,
            if p.hybrid { ", hybrid" } else { "" },
            if p.overridden { ", from config" } else { "" },
        )),
        None => out.push_str("\npreferred: none\n"),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use eloane_hw::{GpuKind, Vendor};

    #[test]
    fn table_marks_preferred_gpu() {
        let gpus = vec![
            Gpu {
                card: "card0".into(),
                pci_address: Some("0000:01:00.0".into()),
                vendor: Vendor::Nvidia,
                vendor_id: 0x10de,
                device_id: 0x28b8,
                driver: Some("nvidia".into()),
                boot_vga: false,
                kind: GpuKind::Discrete,
            },
            Gpu {
                card: "card1".into(),
                pci_address: Some("0000:00:02.0".into()),
                vendor: Vendor::Intel,
                vendor_id: 0x8086,
                device_id: 0xa7a0,
                driver: Some("i915".into()),
                boot_vga: true,
                kind: GpuKind::Integrated,
            },
        ];
        let plan = eloane_hw::select_render_gpu(&gpus, None);
        let table = gpu_table(&gpus, plan.as_ref());
        let lines: Vec<&str> = table.lines().collect();
        assert!(lines[0].contains("CARD"));
        assert!(lines[1].starts_with("*  card0"));
        assert!(lines[2].starts_with("   card1"));
        assert!(table.contains("preferred: card0 (NVIDIA 10de:28b8), hybrid"));
    }
}
