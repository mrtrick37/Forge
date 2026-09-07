//! Native AI/developer Distrobox controller.
//!
//! Rust owns policy, validation, container creation/removal, provisioning,
//! status, and the Ollama lifecycle. The one shell program below is an
//! external provisioning boundary executed inside the managed container;
//! Rust owns when it runs, its inputs, timeout, and failure semantics.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

pub const DEFAULT_BOX: &str = "kyth-ai-dev";
pub const DEFAULT_IMAGE: &str = "registry.fedoraproject.org/fedora-toolbox:44";
pub const DEFAULT_MODEL: &str = "qwen2.5-coder";
pub const DEFAULT_MODEL_DIR_SUFFIX: &str = ".local/share/kyth-ai/models";
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
pub const PROVISION_TIMEOUT: Duration = Duration::from_secs(900);

/// The developer-box package/export policy. It is data owned by the Rust
/// controller, not an independently executable shell entry point.
pub const PROVISION_SCRIPT: &str = r#"
set -euo pipefail
sudo rpm --import https://packages.microsoft.com/keys/microsoft.asc || true
printf '[code]\nname=Visual Studio Code\nbaseurl=https://packages.microsoft.com/yumrepos/vscode\nenabled=1\ngpgcheck=1\ngpgkey=https://packages.microsoft.com/keys/microsoft.asc\n' | sudo tee /etc/yum.repos.d/vscode.repo >/dev/null
printf '[azure-cli]\nname=Azure CLI\nbaseurl=https://packages.microsoft.com/yumrepos/azure-cli\nenabled=1\ngpgcheck=1\ngpgkey=https://packages.microsoft.com/keys/microsoft.asc\n' | sudo tee /etc/yum.repos.d/azure-cli.repo >/dev/null
packages='git git-lfs curl wget jq yq make cmake gcc gcc-c++ python3 python3-pip python3-virtualenv python3-devel nodejs npm rust cargo golang podman skopeo podman-compose vulkan-tools clinfo ollama llama.cpp helix zellij shellcheck shfmt ripgrep fd-find fzf code azure-cli gh flatpak-builder rclone duperemove trivy bat eza fastfetch zoxide evtest lm_sensors i2c-tools v4l-utils hyperfine tmux starship direnv git-delta gum p7zip p7zip-plugins cabextract libpst webkit2gtk4.1-devel javascriptcoregtk4.1-devel libsoup3-devel gtk3-devel dbus-devel'
if command -v dnf5 >/dev/null 2>&1; then pm=dnf5; else pm=dnf; fi
sudo "$pm" install -y --skip-unavailable $packages
sudo "$pm" install -y --skip-unavailable pipx uv zizmor || true
sudo npm install -g @anthropic-ai/claude-code @openai/codex || true
if command -v uv >/dev/null 2>&1; then uv tool install --python 3.13 --upgrade 'headroom-ai[all]' || true; fi
distrobox-export --app code || true
for binary in code az node npm npx hx zellij shellcheck shfmt gh flatpak-builder rclone duperemove trivy zizmor bat eza fastfetch zoxide evtest sensors i2cget i2cset i2cdetect v4l2-ctl jq yq hyperfine tmux pipx uv starship direnv delta gum 7z 7za cabextract readpst claude codex; do
  path="$(command -v "$binary" 2>/dev/null || true)"
  test -z "$path" || distrobox-export --bin "$path" --export-path "$HOME/.local/bin" || true
done
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub box_name: String,
    pub image: String,
    pub model_dir: PathBuf,
}

impl Config {
    pub fn from_environment(environment: &BTreeMap<String, String>) -> Self {
        let home = environment
            .get("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            box_name: environment
                .get("KYTH_AI_DEV_BOX")
                .cloned()
                .unwrap_or_else(|| DEFAULT_BOX.into()),
            image: environment
                .get("KYTH_AI_DEV_IMAGE")
                .cloned()
                .unwrap_or_else(|| DEFAULT_IMAGE.into()),
            model_dir: environment
                .get("KYTH_AI_MODEL_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(DEFAULT_MODEL_DIR_SUFFIX)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuKind {
    Nvidia,
    Amd,
    Dri,
    Cpu,
}

pub fn enter_command(config: &Config, args: &[String]) -> Vec<String> {
    let mut command = vec![
        "distrobox".into(),
        "enter".into(),
        config.box_name.clone(),
        "--".into(),
    ];
    command.extend_from_slice(args);
    command
}

pub fn inside_command(config: &Config, args: &[String]) -> Vec<String> {
    enter_command(config, args)
}

pub fn create_command(config: &Config, git_paths: &[PathBuf], gpu: GpuKind) -> Vec<String> {
    let mut command = vec![
        "distrobox".into(),
        "create".into(),
        "--yes".into(),
        "--name".into(),
        config.box_name.clone(),
        "--image".into(),
        config.image.clone(),
        "--volume".into(),
        volume(&config.model_dir),
    ];
    for git in git_paths {
        command.extend(["--volume".into(), volume_with_mode(git, "rw")]);
        command.extend([
            "--volume".into(),
            volume_with_mode(
                &git.parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(".agents"),
                "rw",
            ),
        ]);
    }
    match gpu {
        GpuKind::Nvidia => command.push("--nvidia".into()),
        GpuKind::Amd | GpuKind::Dri => command.extend([
            "--additional-flags".into(),
            "--device=/dev/kfd --device=/dev/dri --group-add=video --group-add=render".into(),
        ]),
        GpuKind::Cpu => {}
    }
    command
}

fn volume(path: &Path) -> String {
    volume_with_mode(path, "rw")
}
fn volume_with_mode(path: &Path, mode: &str) -> String {
    format!("{}:{}:{mode}", path.display(), path.display())
}

pub fn gpu_description(gpu: GpuKind) -> &'static str {
    match gpu {
        GpuKind::Nvidia => "NVIDIA CUDA detected",
        GpuKind::Amd => "AMD ROCm / HIP detected (/dev/kfd)",
        GpuKind::Dri => "Vulkan / VA-API device detected",
        GpuKind::Cpu => "CPU inference fallback",
    }
}

pub fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn host_command_exists(command: &str) -> bool {
    env::var_os("PATH").is_some_and(|path| {
        env::split_paths(&path).any(|directory| directory.join(command).is_file())
    })
}

pub fn host_git_paths(home: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(output) = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
    {
        if output.status.success() {
            let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !root.is_empty() {
                paths.push(PathBuf::from(root).join(".git"));
            }
        }
    }
    let canonical = home.join("git/kyth/.git");
    if !paths.contains(&canonical) {
        paths.push(canonical);
    }
    paths
}

pub fn box_exists(config: &Config) -> io::Result<bool> {
    let output = run(&["distrobox", "list", "--no-color"], COMMAND_TIMEOUT)?;
    if !output.status.success() {
        return Ok(false);
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.split_whitespace().nth(2) == Some(config.box_name.as_str())))
}

pub fn gpu_kind() -> GpuKind {
    if host_command_exists("nvidia-smi")
        && Command::new("nvidia-smi")
            .arg("-L")
            .output()
            .is_ok_and(|out| out.status.success())
    {
        return GpuKind::Nvidia;
    }
    if is_char_device(Path::new("/dev/kfd")) {
        return GpuKind::Amd;
    }
    if is_char_device(Path::new("/dev/dri/renderD128")) {
        return GpuKind::Dri;
    }
    GpuKind::Cpu
}

fn is_char_device(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.file_type().is_char_device())
}

pub fn create_command_for_host(config: &Config, home: &Path, gpu: GpuKind) -> Vec<String> {
    create_command(config, &host_git_paths(home), gpu)
}

pub fn run(argv: &[&str], timeout: Duration) -> io::Result<Output> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "command must not be empty"))?;
    let mut command = Command::new(program);
    command.args(args);
    run_command(command, timeout)
}

pub fn run_owned(argv: &[String], timeout: Duration) -> io::Result<Output> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "command must not be empty"))?;
    let mut command = Command::new(program);
    command.args(args);
    run_command(command, timeout)
}

pub fn run_command(mut command: Command, timeout: Duration) -> io::Result<Output> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let started = Instant::now();
    loop {
        match child.try_wait()? {
            Some(_) => return child.wait_with_output(),
            None if started.elapsed() <= timeout => std::thread::sleep(Duration::from_millis(25)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "command exceeded its time limit",
                ));
            }
        }
    }
}

pub fn provision_command(config: &Config) -> Vec<String> {
    inside_command(
        config,
        &["bash".into(), "-lc".into(), PROVISION_SCRIPT.into()],
    )
}

pub fn ollama_start_command(config: &Config) -> Vec<String> {
    let model_dir = shell_quote(&config.model_dir.to_string_lossy());
    inside_command(config, &["bash".into(), "-lc".into(), format!(
        "command -v ollama >/dev/null 2>&1 || {{ echo 'ERROR: ollama is not installed. Run: ujust ai-dev-setup' >&2; exit 1; }}; OLLAMA_MODELS={model_dir} nohup ollama serve >/tmp/kyth-ollama.log 2>&1 &",
    )])
}

pub fn ollama_pull_command(config: &Config, model: &str) -> Vec<String> {
    inside_command(
        config,
        &[
            "bash".into(),
            "-lc".into(),
            format!(
                "OLLAMA_MODELS={} ollama pull {}",
                shell_quote(&config.model_dir.to_string_lossy()),
                shell_quote(model),
            ),
        ],
    )
}

pub fn validate_model(model: &str) -> io::Result<()> {
    if model.is_empty()
        || model.len() > 256
        || model
            .bytes()
            .any(|byte| byte == 0 || byte == b'\n' || byte == b'\r')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid model name",
        ));
    }
    Ok(())
}

pub fn remount_paths(paths: &[PathBuf]) {
    for path in paths.iter().flat_map(|git| {
        [
            git.clone(),
            git.parent().unwrap_or(Path::new(".")).join(".agents"),
        ]
    }) {
        let path = path.to_string_lossy().to_string();
        let argv = ["mount", "-o", "remount,rw", path.as_str()];
        let _ = run(&argv, COMMAND_TIMEOUT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_config_from_explicit_environment() {
        let environment = BTreeMap::from([
            ("HOME".into(), "/home/test".into()),
            ("KYTH_AI_DEV_BOX".into(), "work-ai".into()),
            (
                "KYTH_AI_DEV_IMAGE".into(),
                "quay.io/example/dev:latest".into(),
            ),
        ]);
        assert_eq!(
            Config::from_environment(&environment),
            Config {
                box_name: "work-ai".into(),
                image: "quay.io/example/dev:latest".into(),
                model_dir: PathBuf::from("/home/test/.local/share/kyth-ai/models"),
            }
        );
    }

    #[test]
    fn projects_explicit_enter_and_gpu_commands() {
        let config = Config {
            box_name: "kyth-ai-dev".into(),
            image: DEFAULT_IMAGE.into(),
            model_dir: "/home/test/models".into(),
        };
        assert_eq!(
            enter_command(&config, &["node".into(), "--version".into()]),
            vec![
                "distrobox",
                "enter",
                "kyth-ai-dev",
                "--",
                "node",
                "--version",
            ]
        );
        let create = create_command(
            &config,
            &[PathBuf::from("/home/test/git/kyth/.git")],
            GpuKind::Nvidia,
        );
        assert!(create.contains(&"--nvidia".into()));
        assert!(create
            .windows(2)
            .any(|pair| pair[0] == "--image" && pair[1] == DEFAULT_IMAGE));
        assert_eq!(gpu_description(GpuKind::Cpu), "CPU inference fallback");
    }

    #[test]
    fn shell_quote_is_safe_for_container_script_values() {
        assert_eq!(shell_quote("/tmp/models"), "'/tmp/models'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn lifecycle_commands_are_fixed_distrobox_boundaries() {
        let config = Config {
            box_name: "kyth-ai-dev".into(),
            image: DEFAULT_IMAGE.into(),
            model_dir: "/models".into(),
        };
        let setup = provision_command(&config);
        assert_eq!(&setup[..4], &["distrobox", "enter", "kyth-ai-dev", "--"]);
        assert!(ollama_start_command(&config)
            .iter()
            .any(|arg| arg.contains("nohup ollama serve")));
        assert!(ollama_pull_command(&config, "qwen2.5-coder")
            .iter()
            .any(|arg| arg.contains("ollama pull")));
    }
}
