//! GPU detection for Eloane.
//!
//! Enumerates DRM cards through sysfs (`/sys/class/drm/cardN`), classifies them
//! (vendor, integrated vs discrete), detects hybrid laptops (iGPU + dGPU) and
//! picks the preferred render GPU together with the gamescope argument and the
//! PRIME render offload environment needed to use it.
//!
//! The sysfs root is injectable so that the whole detection logic can be tested
//! against a fake tree.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Default sysfs mount point.
pub const DEFAULT_SYSFS_ROOT: &str = "/sys";

/// PCI vendor ids.
pub const VENDOR_NVIDIA: u16 = 0x10de;
pub const VENDOR_AMD: u16 = 0x1002;
pub const VENDOR_INTEL: u16 = 0x8086;

/// PCI vendor ids of virtual/emulated display adapters.
const VIRTUAL_VENDORS: &[u16] = &[
    0x1af4, // Red Hat / virtio-gpu
    0x1234, // QEMU / Bochs stdvga
    0x15ad, // VMware SVGA
    0x80ee, // VirtualBox
    0x1414, // Microsoft Hyper-V
    0x1b36, // Red Hat QXL
];

/// Errors returned by the detection functions.
#[derive(Debug, thiserror::Error)]
pub enum HwError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid value {value:?} in {path}")]
    Parse { path: PathBuf, value: String },
}

/// GPU vendor classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

impl Vendor {
    /// Classifies a PCI vendor id.
    pub fn from_pci_id(id: u16) -> Self {
        match id {
            VENDOR_NVIDIA => Vendor::Nvidia,
            VENDOR_AMD => Vendor::Amd,
            VENDOR_INTEL => Vendor::Intel,
            _ => Vendor::Other,
        }
    }

    /// Human readable vendor name.
    pub fn name(self) -> &'static str {
        match self {
            Vendor::Nvidia => "NVIDIA",
            Vendor::Amd => "AMD",
            Vendor::Intel => "Intel",
            Vendor::Other => "Other",
        }
    }
}

impl fmt::Display for Vendor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Integrated / discrete classification of a GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuKind {
    Integrated,
    Discrete,
    /// Emulated adapter (virtio-gpu, QXL, VMware SVGA, ...).
    Virtual,
    Unknown,
}

impl GpuKind {
    pub fn name(self) -> &'static str {
        match self {
            GpuKind::Integrated => "integrated",
            GpuKind::Discrete => "discrete",
            GpuKind::Virtual => "virtual",
            GpuKind::Unknown => "unknown",
        }
    }

    /// Selection priority, higher is better.
    fn rank(self) -> u8 {
        match self {
            GpuKind::Discrete => 3,
            GpuKind::Integrated => 2,
            GpuKind::Unknown => 1,
            GpuKind::Virtual => 0,
        }
    }
}

impl fmt::Display for GpuKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A GPU found under `class/drm`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gpu {
    /// DRM card name, e.g. `card1`.
    pub card: String,
    /// PCI address, e.g. `0000:01:00.0`.
    pub pci_address: Option<String>,
    pub vendor: Vendor,
    pub vendor_id: u16,
    pub device_id: u16,
    /// Kernel driver name, e.g. `nvidia`, `i915`, `amdgpu`.
    pub driver: Option<String>,
    /// Whether the firmware used this GPU as primary display (`boot_vga`).
    pub boot_vga: bool,
    pub kind: GpuKind,
}

impl Gpu {
    /// `vendor:device` in the format expected by gamescope `--prefer-vk-device`.
    pub fn vk_device_id(&self) -> String {
        format!("{:04x}:{:04x}", self.vendor_id, self.device_id)
    }

    /// Whether this GPU matches a user selector.
    pub fn matches(&self, selector: &GpuSelector) -> bool {
        self.vendor_id == selector.vendor_id
            && selector.device_id.is_none_or(|d| d == self.device_id)
    }
}

/// User override for the render GPU: `vendor:device` (hex, e.g. `10de:28b8`)
/// or just `vendor` (e.g. `10de`). A `0x` prefix is accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct GpuSelector {
    pub vendor_id: u16,
    pub device_id: Option<u16>,
}

impl FromStr for GpuSelector {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parse = |part: &str| {
            parse_hex_u16(part).ok_or_else(|| {
                format!(
                    "invalid GPU selector {s:?}, expected vendor:device in hex (e.g. 10de:28b8)"
                )
            })
        };
        match s.trim().split_once(':') {
            Some((v, d)) => Ok(GpuSelector {
                vendor_id: parse(v)?,
                device_id: Some(parse(d)?),
            }),
            None => Ok(GpuSelector {
                vendor_id: parse(s)?,
                device_id: None,
            }),
        }
    }
}

impl TryFrom<String> for GpuSelector {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<GpuSelector> for String {
    fn from(value: GpuSelector) -> Self {
        value.to_string()
    }
}

impl fmt::Display for GpuSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.device_id {
            Some(d) => write!(f, "{:04x}:{:04x}", self.vendor_id, d),
            None => write!(f, "{:04x}", self.vendor_id),
        }
    }
}

/// The chosen render GPU and how to use it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderPlan {
    /// Selected render GPU.
    pub gpu: Gpu,
    /// Whether the machine has both an integrated and a discrete GPU.
    pub hybrid: bool,
    /// Whether the selection came from a user override.
    pub overridden: bool,
    /// Value for gamescope `--prefer-vk-device`.
    pub prefer_vk_device: String,
    /// PRIME render offload environment (empty when not needed).
    pub env: Vec<(String, String)>,
}

impl RenderPlan {
    /// gamescope arguments selecting the render GPU.
    pub fn gamescope_args(&self) -> Vec<String> {
        vec![
            "--prefer-vk-device".to_string(),
            self.prefer_vk_device.clone(),
        ]
    }
}

/// Detects GPUs under the real `/sys`.
pub fn detect_gpus() -> Result<Vec<Gpu>, HwError> {
    detect_gpus_at(Path::new(DEFAULT_SYSFS_ROOT))
}

/// Detects GPUs under the given sysfs root (`<root>/class/drm/card[0-9]*`).
///
/// Connector entries (`card0-HDMI-A-1`) and render nodes are ignored, as are
/// cards without a readable PCI vendor id. A missing `class/drm` directory
/// yields an empty list. Results are sorted by card index.
pub fn detect_gpus_at(sysfs_root: &Path) -> Result<Vec<Gpu>, HwError> {
    let drm = sysfs_root.join("class/drm");
    let entries = match fs::read_dir(&drm) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(HwError::Io { path: drm, source }),
    };

    let mut cards: Vec<(u32, String)> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| HwError::Io {
            path: drm.clone(),
            source,
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(index) = card_index(&name) {
            cards.push((index, name));
        }
    }
    cards.sort();

    let mut gpus = Vec::new();
    for (_, card) in cards {
        match read_card(&drm.join(&card), &card) {
            Ok(Some(gpu)) => gpus.push(gpu),
            Ok(None) => log::debug!("{card}: no PCI vendor id, skipping"),
            Err(e) => log::warn!("{card}: {e}, skipping"),
        }
    }
    classify(&mut gpus);
    Ok(gpus)
}

/// Returns `Some(n)` for `cardN`, `None` for connectors (`card0-DP-1`) and others.
fn card_index(name: &str) -> Option<u32> {
    let digits = name.strip_prefix("card")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn read_card(card_dir: &Path, card: &str) -> Result<Option<Gpu>, HwError> {
    let device = card_dir.join("device");
    let Some(vendor_id) = read_hex_opt(&device.join("vendor"))? else {
        return Ok(None);
    };
    let device_id = read_hex_opt(&device.join("device"))?.ok_or_else(|| HwError::Parse {
        path: device.join("device"),
        value: String::new(),
    })?;
    let boot_vga = read_trimmed_opt(&device.join("boot_vga"))?.is_some_and(|v| v == "1");
    let driver = fs::read_link(device.join("driver"))
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));

    Ok(Some(Gpu {
        card: card.to_string(),
        pci_address: pci_address(&device)?,
        vendor: Vendor::from_pci_id(vendor_id),
        vendor_id,
        device_id,
        driver,
        boot_vga,
        kind: GpuKind::Unknown,
    }))
}

/// PCI address from `uevent` (`PCI_SLOT_NAME=`), falling back to the name of
/// the resolved `device` symlink.
fn pci_address(device: &Path) -> Result<Option<String>, HwError> {
    if let Some(uevent) = read_trimmed_opt(&device.join("uevent"))? {
        if let Some(slot) = uevent
            .lines()
            .find_map(|l| l.strip_prefix("PCI_SLOT_NAME="))
        {
            return Ok(Some(slot.trim().to_string()));
        }
    }
    Ok(fs::canonicalize(device)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .filter(|n| looks_like_pci_address(n)))
}

fn looks_like_pci_address(s: &str) -> bool {
    // dddd:bb:dd.f
    let b = s.as_bytes();
    b.len() == 12
        && b[4] == b':'
        && b[7] == b':'
        && b[10] == b'.'
        && s.chars()
            .enumerate()
            .all(|(i, c)| matches!(i, 4 | 7 | 10) || c.is_ascii_hexdigit())
}

/// Assigns [`GpuKind`] to every GPU.
///
/// Heuristic: virtual adapters are recognised by vendor id; Intel is
/// integrated; NVIDIA is discrete; AMD is discrete unless it is the boot GPU
/// while another non-boot NVIDIA/AMD GPU exists (AMD APU + dGPU laptop).
fn classify(gpus: &mut [Gpu]) {
    let other_non_boot_dgpu = |idx: usize, gpus: &[Gpu]| {
        gpus.iter().enumerate().any(|(i, g)| {
            i != idx && !g.boot_vga && matches!(g.vendor, Vendor::Nvidia | Vendor::Amd)
        })
    };
    let kinds: Vec<GpuKind> = gpus
        .iter()
        .enumerate()
        .map(|(i, g)| {
            if VIRTUAL_VENDORS.contains(&g.vendor_id) {
                return GpuKind::Virtual;
            }
            match g.vendor {
                Vendor::Intel => GpuKind::Integrated,
                Vendor::Nvidia => GpuKind::Discrete,
                Vendor::Amd if g.boot_vga && other_non_boot_dgpu(i, gpus) => GpuKind::Integrated,
                Vendor::Amd => GpuKind::Discrete,
                Vendor::Other => GpuKind::Unknown,
            }
        })
        .collect();
    for (gpu, kind) in gpus.iter_mut().zip(kinds) {
        gpu.kind = kind;
    }
}

/// Whether the GPU list describes a hybrid machine (iGPU + dGPU).
pub fn is_hybrid(gpus: &[Gpu]) -> bool {
    gpus.iter().any(|g| g.kind == GpuKind::Integrated)
        && gpus.iter().any(|g| g.kind == GpuKind::Discrete)
}

/// Picks the preferred render GPU.
///
/// A matching `prefer` override wins; otherwise discrete beats integrated,
/// which beats unknown and virtual adapters; ties go to the boot GPU, then to
/// the lowest card index. Returns `None` when `gpus` is empty.
///
/// PRIME offload variables are produced when the machine is hybrid and the
/// selected GPU is a discrete one (i.e. not the display GPU).
pub fn select_render_gpu(gpus: &[Gpu], prefer: Option<&GpuSelector>) -> Option<RenderPlan> {
    let overridden = prefer.and_then(|sel| gpus.iter().find(|g| g.matches(sel)));
    if let (Some(sel), None) = (prefer, overridden) {
        log::warn!("preferred GPU {sel} not found, falling back to automatic selection");
    }
    let gpu = match overridden {
        Some(g) => g,
        // `max_by_key` returns the last max; iterate in reverse so the lowest
        // card index wins ties.
        None => gpus
            .iter()
            .rev()
            .max_by_key(|g| (g.kind.rank(), g.boot_vga))?,
    };

    let hybrid = is_hybrid(gpus);
    let env = if hybrid && gpu.kind == GpuKind::Discrete {
        prime_env(gpu.vendor)
    } else {
        Vec::new()
    };

    Some(RenderPlan {
        gpu: gpu.clone(),
        hybrid,
        overridden: overridden.is_some(),
        prefer_vk_device: gpu.vk_device_id(),
        env,
    })
}

/// PRIME render offload environment for a vendor.
pub fn prime_env(vendor: Vendor) -> Vec<(String, String)> {
    let pairs: &[(&str, &str)] = match vendor {
        Vendor::Nvidia => &[
            ("__NV_PRIME_RENDER_OFFLOAD", "1"),
            ("__GLX_VENDOR_LIBRARY_NAME", "nvidia"),
            ("__VK_LAYER_NV_optimus", "NVIDIA_only"),
        ],
        Vendor::Amd | Vendor::Intel | Vendor::Other => &[("DRI_PRIME", "1")],
    };
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn parse_hex_u16(s: &str) -> Option<u16> {
    let s = s.trim();
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    if s.is_empty() || s.len() > 4 {
        return None;
    }
    u16::from_str_radix(s, 16).ok()
}

fn read_trimmed_opt(path: &Path) -> Result<Option<String>, HwError> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(Some(s.trim().to_string())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(HwError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn read_hex_opt(path: &Path) -> Result<Option<u16>, HwError> {
    match read_trimmed_opt(path)? {
        None => Ok(None),
        Some(value) => parse_hex_u16(&value).map(Some).ok_or(HwError::Parse {
            path: path.to_path_buf(),
            value,
        }),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    struct FakeCard<'a> {
        card: u32,
        pci: &'a str,
        vendor: u16,
        device: u16,
        boot_vga: Option<bool>,
        driver: &'a str,
    }

    /// Builds a sysfs-like tree mirroring the real layout:
    /// `class/drm/cardN -> devices/pci0000:00/<pci>/drm/cardN`, with
    /// `<pci>/driver -> bus/pci/drivers/<driver>`.
    fn fake_sysfs(cards: &[FakeCard]) -> TempDir {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        fs::create_dir_all(r.join("class/drm")).unwrap();
        for c in cards {
            let pci_dir = r.join("devices/pci0000:00").join(c.pci);
            let card_dir = pci_dir.join("drm").join(format!("card{}", c.card));
            fs::create_dir_all(&card_dir).unwrap();
            fs::write(pci_dir.join("vendor"), format!("0x{:04x}\n", c.vendor)).unwrap();
            fs::write(pci_dir.join("device"), format!("0x{:04x}\n", c.device)).unwrap();
            if let Some(b) = c.boot_vga {
                fs::write(pci_dir.join("boot_vga"), if b { "1\n" } else { "0\n" }).unwrap();
            }
            let drv = r.join("bus/pci/drivers").join(c.driver);
            fs::create_dir_all(&drv).unwrap();
            symlink(&drv, pci_dir.join("driver")).unwrap();
            symlink(&pci_dir, card_dir.join("device")).unwrap();
            symlink(
                &card_dir,
                r.join("class/drm").join(format!("card{}", c.card)),
            )
            .unwrap();
            // Connector and render node entries that must be ignored.
            let connector = card_dir.join(format!("card{}-HDMI-A-1", c.card));
            fs::create_dir_all(&connector).unwrap();
            symlink(
                &connector,
                r.join("class/drm").join(format!("card{}-HDMI-A-1", c.card)),
            )
            .unwrap();
            fs::create_dir_all(r.join("class/drm").join(format!("renderD{}", 128 + c.card)))
                .unwrap();
        }
        fs::write(r.join("class/drm/version"), "drm 1.1.0 20060810\n").unwrap();
        root
    }

    fn hybrid_intel_nvidia() -> TempDir {
        fake_sysfs(&[
            FakeCard {
                card: 0,
                pci: "0000:01:00.0",
                vendor: 0x10de,
                device: 0x28b8,
                boot_vga: Some(false),
                driver: "nvidia",
            },
            FakeCard {
                card: 1,
                pci: "0000:00:02.0",
                vendor: 0x8086,
                device: 0xa7a0,
                boot_vga: Some(true),
                driver: "i915",
            },
        ])
    }

    #[test]
    fn hybrid_intel_nvidia_prefers_dgpu_with_prime_env() {
        let root = hybrid_intel_nvidia();
        let gpus = detect_gpus_at(root.path()).unwrap();
        assert_eq!(gpus.len(), 2, "connectors/render nodes must be ignored");

        let nv = &gpus[0];
        assert_eq!(nv.card, "card0");
        assert_eq!(nv.vendor, Vendor::Nvidia);
        assert_eq!(nv.kind, GpuKind::Discrete);
        assert_eq!(nv.driver.as_deref(), Some("nvidia"));
        assert_eq!(nv.pci_address.as_deref(), Some("0000:01:00.0"));
        assert!(!nv.boot_vga);

        let intel = &gpus[1];
        assert_eq!(intel.vendor, Vendor::Intel);
        assert_eq!(intel.kind, GpuKind::Integrated);
        assert_eq!(intel.driver.as_deref(), Some("i915"));
        assert!(intel.boot_vga);

        assert!(is_hybrid(&gpus));
        let plan = select_render_gpu(&gpus, None).unwrap();
        assert_eq!(plan.gpu.card, "card0");
        assert!(plan.hybrid);
        assert!(!plan.overridden);
        assert_eq!(plan.prefer_vk_device, "10de:28b8");
        assert_eq!(plan.gamescope_args(), ["--prefer-vk-device", "10de:28b8"]);
        let env: Vec<(&str, &str)> = plan
            .env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        assert_eq!(
            env,
            [
                ("__NV_PRIME_RENDER_OFFLOAD", "1"),
                ("__GLX_VENDOR_LIBRARY_NAME", "nvidia"),
                ("__VK_LAYER_NV_optimus", "NVIDIA_only"),
            ]
        );
    }

    #[test]
    fn override_selects_igpu_without_prime_env() {
        let root = hybrid_intel_nvidia();
        let gpus = detect_gpus_at(root.path()).unwrap();
        let sel: GpuSelector = "0x8086:0xA7A0".parse().unwrap();
        let plan = select_render_gpu(&gpus, Some(&sel)).unwrap();
        assert_eq!(plan.gpu.vendor, Vendor::Intel);
        assert!(plan.overridden);
        assert!(plan.hybrid);
        assert_eq!(plan.prefer_vk_device, "8086:a7a0");
        assert!(plan.env.is_empty());
    }

    #[test]
    fn unknown_override_falls_back_to_auto() {
        let root = hybrid_intel_nvidia();
        let gpus = detect_gpus_at(root.path()).unwrap();
        let sel: GpuSelector = "1002".parse().unwrap();
        let plan = select_render_gpu(&gpus, Some(&sel)).unwrap();
        assert!(!plan.overridden);
        assert_eq!(plan.gpu.vendor, Vendor::Nvidia);
    }

    #[test]
    fn amd_only() {
        let root = fake_sysfs(&[FakeCard {
            card: 0,
            pci: "0000:03:00.0",
            vendor: 0x1002,
            device: 0x744c,
            boot_vga: Some(true),
            driver: "amdgpu",
        }]);
        let gpus = detect_gpus_at(root.path()).unwrap();
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].vendor, Vendor::Amd);
        assert_eq!(gpus[0].kind, GpuKind::Discrete);
        assert!(!is_hybrid(&gpus));
        let plan = select_render_gpu(&gpus, None).unwrap();
        assert_eq!(plan.prefer_vk_device, "1002:744c");
        assert!(!plan.hybrid);
        assert!(plan.env.is_empty());
    }

    #[test]
    fn amd_apu_with_amd_dgpu_uses_dri_prime() {
        let root = fake_sysfs(&[
            FakeCard {
                card: 0,
                pci: "0000:05:00.0",
                vendor: 0x1002,
                device: 0x15bf,
                boot_vga: Some(true),
                driver: "amdgpu",
            },
            FakeCard {
                card: 1,
                pci: "0000:03:00.0",
                vendor: 0x1002,
                device: 0x7480,
                boot_vga: Some(false),
                driver: "amdgpu",
            },
        ]);
        let gpus = detect_gpus_at(root.path()).unwrap();
        assert_eq!(gpus[0].kind, GpuKind::Integrated);
        assert_eq!(gpus[1].kind, GpuKind::Discrete);
        let plan = select_render_gpu(&gpus, None).unwrap();
        assert_eq!(plan.gpu.card, "card1");
        assert_eq!(plan.env, [("DRI_PRIME".to_string(), "1".to_string())]);
    }

    #[test]
    fn vm_with_virtio_gpu() {
        let root = fake_sysfs(&[FakeCard {
            card: 0,
            pci: "0000:00:01.0",
            vendor: 0x1af4,
            device: 0x1050,
            boot_vga: None,
            driver: "virtio-pci",
        }]);
        let gpus = detect_gpus_at(root.path()).unwrap();
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].vendor, Vendor::Other);
        assert_eq!(gpus[0].kind, GpuKind::Virtual);
        assert!(!gpus[0].boot_vga);
        assert!(!is_hybrid(&gpus));
        let plan = select_render_gpu(&gpus, None).unwrap();
        assert_eq!(plan.prefer_vk_device, "1af4:1050");
        assert!(plan.env.is_empty());
    }

    #[test]
    fn missing_drm_class_yields_no_gpu() {
        let root = tempfile::tempdir().unwrap();
        let gpus = detect_gpus_at(root.path()).unwrap();
        assert!(gpus.is_empty());
        assert!(select_render_gpu(&gpus, None).is_none());
    }

    #[test]
    fn card_name_filter() {
        assert_eq!(card_index("card0"), Some(0));
        assert_eq!(card_index("card12"), Some(12));
        assert_eq!(card_index("card0-HDMI-A-1"), None);
        assert_eq!(card_index("card"), None);
        assert_eq!(card_index("renderD128"), None);
    }

    #[test]
    fn selector_parsing_and_serde() {
        let s: GpuSelector = "10de:28b8".parse().unwrap();
        assert_eq!(s.vendor_id, 0x10de);
        assert_eq!(s.device_id, Some(0x28b8));
        assert_eq!(s.to_string(), "10de:28b8");
        assert!("nvidia".parse::<GpuSelector>().is_err());
        assert!("10de:".parse::<GpuSelector>().is_err());
        assert!("123456".parse::<GpuSelector>().is_err());

        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"10de:28b8\"");
        let back: GpuSelector = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn gpu_serializes() {
        let root = hybrid_intel_nvidia();
        let gpus = detect_gpus_at(root.path()).unwrap();
        let json = serde_json::to_value(&gpus[0]).unwrap();
        assert_eq!(json["vendor"], "nvidia");
        assert_eq!(json["kind"], "discrete");
        assert_eq!(json["vendor_id"], 0x10de);
    }
}
