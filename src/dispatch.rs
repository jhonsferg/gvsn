//! Command dispatch trait and registry.
//!
//! Provides an extensible command system that follows the Open/Closed Principle -
//! adding new commands doesn't require modifying `main.rs`.

use anyhow::Result;

use crate::{
    commands::{
        build, completions, current, default, doctor, env, exec, implode, install, list,
        list_remote, local_version, outdated, path, prune, setup, shell, uninstall, upgrade,
        use_version,
    },
    config::{Config, ConfigMut},
    http::HttpClient,
};

/// Trait for all gvsn commands.
///
/// Each command implements this trait to provide its name, description,
/// and execution logic. Commands are registered in the global registry
/// at startup via [`register_commands`].
pub trait Command: Send + Sync + std::fmt::Debug {
    /// Command name (e.g., "install", "use", "build").
    fn name(&self) -> &'static str;

    /// Whether this command requires an HTTP client.
    fn needs_http(&self) -> bool {
        true
    }

    /// Executes the command.
    ///
    /// # Arguments
    /// - `config`: Read-only access to configuration (paths, etc.)
    /// - `config_mut`: Mutable access for commands that need to create directories
    /// - `client`: HTTP client for network operations (None if `needs_http` is false)
    /// - `args`: Command-specific arguments parsed by clap
    fn execute(
        &self,
        config: &Config,
        config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()>;
}

/// Global command registry.
///
/// Thread-safe registry that stores all registered commands.
/// Initialized once at startup via [`register_commands`].
#[derive(Debug)]
pub struct CommandRegistry {
    commands: std::collections::HashMap<String, Box<dyn Command>>,
}

impl CommandRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self {
            commands: std::collections::HashMap::new(),
        }
    }

    /// Registers a command in the registry.
    pub fn register(&mut self, command: Box<dyn Command>) {
        let name = command.name().to_string();
        self.commands.insert(name, command);
    }

    /// Dispatches a command by name.
    ///
    /// # Arguments
    /// - `name`: Command name to dispatch
    /// - `config`: Read-only config reference
    /// - `config_mut`: Mutable config reference (for directory creation)
    /// - `client`: HTTP client (optional)
    /// - `args`: Command arguments
    ///
    /// # Errors
    /// Returns an error if the command is not found or execution fails.
    pub fn dispatch(
        &self,
        name: &str,
        config: &Config,
        config_mut: &dyn ConfigMut,
        client: &HttpClient,
        args: Vec<String>,
    ) -> anyhow::Result<()> {
        let command = self
            .commands
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Unknown command: {}", name))?;
        let client_opt = if command.needs_http() {
            Some(client)
        } else {
            None
        };
        command.execute(config, config_mut, client_opt, args)
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Command implementations
// =============================================================================

#[derive(Debug)]
struct InstallCmd;

impl Command for InstallCmd {
    fn name(&self) -> &'static str {
        "install"
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let client = client.expect("install requires HTTP client");
        let (spec_str, force) = parse_install_args(&args);
        install::run(config, client, &spec_str, force)
    }
}

fn parse_install_args(args: &[String]) -> (String, bool) {
    let mut spec_str = String::new();
    let mut force = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--force" | "-f" => force = true,
            "--retries" => {
                i += 1; // skip value
            }
            _ if !args[i].starts_with('-') => {
                spec_str = args[i].clone();
            }
            _ => {}
        }
        i += 1;
    }
    (spec_str, force)
}

#[derive(Debug)]
struct UseCmd;

impl Command for UseCmd {
    fn name(&self) -> &'static str {
        "use"
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let _ = client;
        let spec_str = args.first().cloned().unwrap_or_default();
        use_version::run(config, &spec_str)
    }
}

#[derive(Debug)]
struct DefaultCmd;

impl Command for DefaultCmd {
    fn name(&self) -> &'static str {
        "default"
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let _ = client;
        let spec_str = args.first().cloned().unwrap_or_default();
        default::run(config, &spec_str)
    }
}

#[derive(Debug)]
struct LocalVersionCmd;

impl Command for LocalVersionCmd {
    fn name(&self) -> &'static str {
        "local"
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let _ = client;
        let spec_str = args.first().cloned().unwrap_or_default();
        local_version::run(config, &spec_str)
    }
}

#[derive(Debug)]
struct UninstallCmd;

impl Command for UninstallCmd {
    fn name(&self) -> &'static str {
        "uninstall"
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let _ = client;
        let (spec_str, force) = parse_install_args(&args);
        uninstall::run(config, &spec_str, force)
    }
}

#[derive(Debug)]
struct ListCmd;

impl Command for ListCmd {
    fn name(&self) -> &'static str {
        "list"
    }
    fn needs_http(&self) -> bool {
        false
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let _ = client;
        let json = args.iter().any(|a| a == "--json");
        list::run(config, json)
    }
}

#[derive(Debug)]
struct ListRemoteCmd;

impl Command for ListRemoteCmd {
    fn name(&self) -> &'static str {
        "list-remote"
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let client = client.expect("list-remote requires HTTP client");
        let mut all = false;
        let mut json = false;
        for arg in &args {
            match arg.as_str() {
                "--all" => all = true,
                "--json" => json = true,
                _ => {}
            }
        }
        list_remote::run(config, client, all, json)
    }
}

#[derive(Debug)]
struct CurrentCmd;

impl Command for CurrentCmd {
    fn name(&self) -> &'static str {
        "current"
    }
    fn needs_http(&self) -> bool {
        false
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let _ = client;
        let _ = args;
        current::run(config)
    }
}

#[derive(Debug)]
struct PathCmd;

impl Command for PathCmd {
    fn name(&self) -> &'static str {
        "path"
    }
    fn needs_http(&self) -> bool {
        false
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let _ = client;
        let spec_str = args.first().cloned();
        path::run(config, spec_str.as_deref())
    }
}

#[derive(Debug)]
struct EnvCmd;

impl Command for EnvCmd {
    fn name(&self) -> &'static str {
        "env"
    }
    fn needs_http(&self) -> bool {
        false
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let _ = client;
        let shell_str = args.first().cloned();
        env::run(config, shell_str.as_deref())
    }
}

#[derive(Debug)]
struct SetupCmd;

impl Command for SetupCmd {
    fn name(&self) -> &'static str {
        "setup"
    }
    fn needs_http(&self) -> bool {
        false
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let _ = config;
        let _ = client;
        let mut shell_str = None;
        let mut reset = false;
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--shell" => {
                    i += 1;
                    if i < args.len() {
                        shell_str = Some(args[i].clone());
                    }
                }
                "--reset" => reset = true,
                _ => {}
            }
            i += 1;
        }
        setup::run(shell_str.as_deref(), reset)
    }
}

#[derive(Debug)]
struct ExecCmd;

impl Command for ExecCmd {
    fn name(&self) -> &'static str {
        "exec"
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let _ = client;
        if args.is_empty() {
            return Err(anyhow::anyhow!("exec requires a version and command"));
        }
        let spec_str = args[0].clone();
        let cmd_args = args[1..].to_vec();
        exec::run(config, &spec_str, &cmd_args)
    }
}

#[derive(Debug)]
struct DoctorCmd;

impl Command for DoctorCmd {
    fn name(&self) -> &'static str {
        "doctor"
    }
    fn needs_http(&self) -> bool {
        false
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let _ = client;
        let mut shell_str = None;
        let mut fix = false;
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--shell" => {
                    i += 1;
                    if i < args.len() {
                        shell_str = Some(args[i].clone());
                    }
                }
                "--fix" => fix = true,
                _ => {}
            }
            i += 1;
        }
        doctor::run(config, shell_str.as_deref(), fix)
    }
}

#[derive(Debug)]
struct CompletionsCmd;

impl Command for CompletionsCmd {
    fn name(&self) -> &'static str {
        "completions"
    }
    fn needs_http(&self) -> bool {
        false
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let _ = config;
        let _ = client;
        let shell_str = args.first().cloned().unwrap_or_default();
        completions::run(&shell_str)
    }
}

#[derive(Debug)]
struct OutdatedCmd;

impl Command for OutdatedCmd {
    fn name(&self) -> &'static str {
        "outdated"
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let json = args.iter().any(|a| a == "--json");
        let client = client.expect("outdated requires HTTP client");
        outdated::run(config, client, json)
    }
}

#[derive(Debug)]
struct PruneCmd;

impl Command for PruneCmd {
    fn name(&self) -> &'static str {
        "prune"
    }
    fn needs_http(&self) -> bool {
        false
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let _ = client;
        let mut force = false;
        let mut dry_run = false;
        let mut scan_dir = None;
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--force" => force = true,
                "--dry-run" => dry_run = true,
                "--scan-dir" => {
                    i += 1;
                    if i < args.len() {
                        scan_dir = Some(args[i].clone());
                    }
                }
                _ => {}
            }
            i += 1;
        }
        prune::run(config, force, dry_run, scan_dir.as_deref())
    }
}

#[derive(Debug)]
struct ShellCmd;

impl Command for ShellCmd {
    fn name(&self) -> &'static str {
        "shell"
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let _ = client;
        let mut spec_str = None;
        let mut unset = false;
        let mut shell_str = None;
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--unset" => unset = true,
                "--shell" => {
                    i += 1;
                    if i < args.len() {
                        shell_str = Some(args[i].clone());
                    }
                }
                _ if !args[i].starts_with('-') => {
                    spec_str = Some(args[i].clone());
                }
                _ => {}
            }
            i += 1;
        }
        shell::run(config, spec_str.as_deref(), unset, shell_str.as_deref())
    }
}

#[derive(Debug)]
struct UpgradeCmd;

impl Command for UpgradeCmd {
    fn name(&self) -> &'static str {
        "upgrade"
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let client = client.expect("upgrade requires HTTP client");
        let mut force = false;
        for arg in &args {
            if arg == "--force" {
                force = true;
            }
        }
        upgrade::run(config, client, force)
    }
}

#[derive(Debug)]
struct ImplodeCmd;

impl Command for ImplodeCmd {
    fn name(&self) -> &'static str {
        "implode"
    }
    fn needs_http(&self) -> bool {
        false
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let _ = client;
        let mut force = false;
        for arg in &args {
            if arg == "--force" {
                force = true;
            }
        }
        implode::run(config, force)
    }
}

#[derive(Debug)]
struct BuildCmd;

impl Command for BuildCmd {
    fn name(&self) -> &'static str {
        "build"
    }
    fn execute(
        &self,
        config: &Config,
        _config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()> {
        let client = client.expect("build requires HTTP client");
        let mut version = String::new();
        let mut force = false;
        let mut no_cgo = false;
        let mut bootstrap = None;
        let mut env_vars = Vec::new();

        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--force" | "-f" => force = true,
                "--no-cgo" => no_cgo = true,
                "--bootstrap" => {
                    i += 1;
                    if i < args.len() {
                        bootstrap = Some(args[i].clone());
                    }
                }
                "--env" => {
                    i += 1;
                    if i < args.len() {
                        env_vars.push(args[i].clone());
                    }
                }
                "--retries" => {
                    // Consumed here only to skip its value - the actual
                    // retry count already flows through the shared
                    // `HttpClient` built in `main.rs` before dispatch.
                    i += 1;
                }
                _ if !args[i].starts_with('-') => {
                    version = args[i].clone();
                }
                _ => {}
            }
            i += 1;
        }
        build::run(
            config,
            client,
            &version,
            force,
            no_cgo,
            bootstrap.as_deref(),
            &env_vars,
        )
    }
}

/// Converts a clap Command enum to a command name string and arguments vector.
pub fn command_to_name_and_args(cmd: &crate::cli::Command) -> (String, Vec<String>) {
    // Helper to extract DownloadArgs fields
    fn extract_download_args(download: &crate::cli::DownloadArgs) -> Vec<String> {
        let mut args = Vec::new();
        if download.retries != 3 {
            args.push("--retries".to_string());
            args.push(download.retries.to_string());
        }
        args
    }

    use crate::cli::Command::*;

    match cmd {
        Build {
            version,
            force,
            no_cgo,
            bootstrap,
            env_vars,
            download,
        } => {
            let mut args = vec![version.clone()];
            if *force {
                args.push("--force".to_string());
            }
            if *no_cgo {
                args.push("--no-cgo".to_string());
            }
            if let Some(b) = bootstrap {
                args.push("--bootstrap".to_string());
                args.push(b.clone());
            }
            for e in env_vars {
                args.push("--env".to_string());
                args.push(e.clone());
            }
            args.extend(extract_download_args(download));
            ("build".to_string(), args)
        }
        Install {
            version,
            force,
            download,
        } => {
            let mut args = vec![version.clone()];
            if *force {
                args.push("--force".to_string());
            }
            args.extend(extract_download_args(download));
            ("install".to_string(), args)
        }
        Use { version } => ("use".to_string(), vec![version.clone()]),
        Default { version } => ("default".to_string(), vec![version.clone()]),
        Local { version } => ("local".to_string(), vec![version.clone()]),
        Uninstall { version, force } => {
            let mut args = vec![version.clone()];
            if *force {
                args.push("--force".to_string());
            }
            ("uninstall".to_string(), args)
        }
        List { json } => {
            let mut args = vec![];
            if *json {
                args.push("--json".to_string());
            }
            ("list".to_string(), args)
        }
        ListRemote { all, json } => {
            let mut args = vec![];
            if *all {
                args.push("--all".to_string());
            }
            if *json {
                args.push("--json".to_string());
            }
            ("list-remote".to_string(), args)
        }
        Current => ("current".to_string(), vec![]),
        Path { version } => {
            let mut args = vec![];
            if let Some(v) = version {
                args.push(v.clone());
            }
            ("path".to_string(), args)
        }
        Env { shell } => {
            let mut args = vec![];
            if let Some(s) = shell {
                args.push(s.clone());
            }
            ("env".to_string(), args)
        }
        Setup { shell, reset } => {
            let mut args = vec![];
            if let Some(s) = shell {
                args.push("--shell".to_string());
                args.push(s.clone());
            }
            if *reset {
                args.push("--reset".to_string());
            }
            ("setup".to_string(), args)
        }
        Exec {
            version,
            args: cmd_args,
        } => {
            let mut args = vec![version.clone()];
            args.extend(cmd_args.clone());
            ("exec".to_string(), args)
        }
        Doctor { shell, fix } => {
            let mut args = vec![];
            if let Some(s) = shell {
                args.push("--shell".to_string());
                args.push(s.clone());
            }
            if *fix {
                args.push("--fix".to_string());
            }
            ("doctor".to_string(), args)
        }
        Completions { shell } => ("completions".to_string(), vec![shell.clone()]),
        Upgrade { force, download: _ } => {
            let mut args = vec![];
            if *force {
                args.push("--force".to_string());
            }
            ("upgrade".to_string(), args)
        }
        Implode { force } => {
            let mut args = vec![];
            if *force {
                args.push("--force".to_string());
            }
            ("implode".to_string(), args)
        }
        Outdated { json } => {
            let mut args = vec![];
            if *json {
                args.push("--json".to_string());
            }
            ("outdated".to_string(), args)
        }
        Prune {
            force,
            dry_run,
            scan_dir,
        } => {
            let mut args = vec![];
            if *force {
                args.push("--force".to_string());
            }
            if *dry_run {
                args.push("--dry-run".to_string());
            }
            if let Some(s) = scan_dir {
                args.push("--scan-dir".to_string());
                args.push(s.clone());
            }
            ("prune".to_string(), args)
        }
        Shell {
            version,
            unset,
            shell,
        } => {
            let mut args = vec![];
            if let Some(v) = version {
                args.push(v.clone());
            }
            if *unset {
                args.push("--unset".to_string());
            }
            if let Some(s) = shell {
                args.push("--shell".to_string());
                args.push(s.clone());
            }
            ("shell".to_string(), args)
        }
    }
}

/// Builds the complete command registry with all built-in commands.
fn build_registry() -> CommandRegistry {
    let mut registry = CommandRegistry::new();

    // Register all built-in commands
    registry.register(Box::new(InstallCmd));
    registry.register(Box::new(UseCmd));
    registry.register(Box::new(DefaultCmd));
    registry.register(Box::new(LocalVersionCmd));
    registry.register(Box::new(UninstallCmd));
    registry.register(Box::new(ListCmd));
    registry.register(Box::new(ListRemoteCmd));
    registry.register(Box::new(CurrentCmd));
    registry.register(Box::new(PathCmd));
    registry.register(Box::new(EnvCmd));
    registry.register(Box::new(SetupCmd));
    registry.register(Box::new(ExecCmd));
    registry.register(Box::new(DoctorCmd));
    registry.register(Box::new(CompletionsCmd));
    registry.register(Box::new(OutdatedCmd));
    registry.register(Box::new(PruneCmd));
    registry.register(Box::new(ShellCmd));
    registry.register(Box::new(UpgradeCmd));
    registry.register(Box::new(ImplodeCmd));
    registry.register(Box::new(BuildCmd));

    registry
}

/// Free function to dispatch a command using the built-in registry.
/// This is the main entry point for command dispatch from main.rs.
pub fn dispatch(
    config: &crate::config::Config,
    config_mut: &dyn crate::config::ConfigMut,
    client: &crate::http::HttpClient,
    name: String,
    args: Vec<String>,
) -> anyhow::Result<()> {
    let registry = build_registry();
    registry.dispatch(&name, config, config_mut, client, args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Command as CliCommand, DownloadArgs};

    fn download_args(retries: u8) -> DownloadArgs {
        DownloadArgs { retries }
    }

    // ---- parse_install_args -------------------------------------------------

    #[test]
    fn parse_install_args_extracts_version_only() {
        let (spec, force) = parse_install_args(&["1.22.4".to_string()]);
        assert_eq!(spec, "1.22.4");
        assert!(!force);
    }

    #[test]
    fn parse_install_args_detects_force_short_and_long() {
        let (spec, force) = parse_install_args(&["1.22.4".to_string(), "--force".to_string()]);
        assert_eq!(spec, "1.22.4");
        assert!(force);

        let (spec, force) = parse_install_args(&["1.22.4".to_string(), "-f".to_string()]);
        assert_eq!(spec, "1.22.4");
        assert!(force);
    }

    #[test]
    fn parse_install_args_skips_retries_value() {
        let (spec, force) = parse_install_args(&[
            "1.22.4".to_string(),
            "--retries".to_string(),
            "5".to_string(),
        ]);
        assert_eq!(spec, "1.22.4");
        assert!(!force);
    }

    #[test]
    fn parse_install_args_empty_args_yields_empty_spec() {
        let (spec, force) = parse_install_args(&[]);
        assert_eq!(spec, "");
        assert!(!force);
    }

    #[test]
    fn parse_install_args_last_positional_wins() {
        // Only the last non-flag argument should be kept as the spec.
        let (spec, _) = parse_install_args(&["1.21".to_string(), "1.22.4".to_string()]);
        assert_eq!(spec, "1.22.4");
    }

    // ---- CommandRegistry ------------------------------------------------------

    #[test]
    fn registry_dispatch_unknown_command_errors() {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::config::Config {
            root: dir.path().to_path_buf(),
        };
        let client = crate::http::HttpClient::new(false, 3).unwrap();
        let registry = build_registry();

        let err = registry
            .dispatch("does-not-exist", &config, &config, &client, vec![])
            .unwrap_err();
        assert!(err.to_string().contains("Unknown command"));
    }

    #[test]
    fn registry_contains_all_built_in_commands() {
        let registry = build_registry();
        for name in [
            "install",
            "use",
            "default",
            "local",
            "uninstall",
            "list",
            "list-remote",
            "current",
            "path",
            "env",
            "setup",
            "exec",
            "doctor",
            "completions",
            "outdated",
            "prune",
            "shell",
            "upgrade",
            "implode",
            "build",
        ] {
            assert!(
                registry.commands.contains_key(name),
                "missing command: {name}"
            );
        }
    }

    #[test]
    fn needs_http_false_for_local_only_commands() {
        let registry = build_registry();
        for name in [
            "list",
            "current",
            "path",
            "env",
            "setup",
            "doctor",
            "completions",
            "prune",
            "implode",
        ] {
            let cmd = registry.commands.get(name).unwrap();
            assert!(!cmd.needs_http(), "{name} should not need http");
        }
    }

    #[test]
    fn needs_http_true_by_default_for_network_commands() {
        let registry = build_registry();
        for name in ["install", "list-remote", "outdated", "upgrade", "build"] {
            let cmd = registry.commands.get(name).unwrap();
            assert!(cmd.needs_http(), "{name} should need http");
        }
    }

    // ---- command_to_name_and_args ---------------------------------------------

    #[test]
    fn convert_build_with_all_options() {
        let cmd = CliCommand::Build {
            version: "1.22.4".to_string(),
            force: true,
            no_cgo: true,
            bootstrap: Some("1.20".to_string()),
            env_vars: vec!["FOO=bar".to_string()],
            download: download_args(3),
        };
        let (name, args) = command_to_name_and_args(&cmd);
        assert_eq!(name, "build");
        assert_eq!(
            args,
            vec![
                "1.22.4".to_string(),
                "--force".to_string(),
                "--no-cgo".to_string(),
                "--bootstrap".to_string(),
                "1.20".to_string(),
                "--env".to_string(),
                "FOO=bar".to_string(),
            ]
        );
    }

    #[test]
    fn convert_build_minimal() {
        let cmd = CliCommand::Build {
            version: "1.22.4".to_string(),
            force: false,
            no_cgo: false,
            bootstrap: None,
            env_vars: vec![],
            download: download_args(3),
        };
        let (name, args) = command_to_name_and_args(&cmd);
        assert_eq!(name, "build");
        assert_eq!(args, vec!["1.22.4".to_string()]);
    }

    #[test]
    fn convert_build_includes_retries_when_non_default() {
        let cmd = CliCommand::Build {
            version: "1.22.4".to_string(),
            force: false,
            no_cgo: false,
            bootstrap: None,
            env_vars: vec![],
            download: download_args(7),
        };
        let (_, args) = command_to_name_and_args(&cmd);
        assert!(args.contains(&"--retries".to_string()));
        assert!(args.contains(&"7".to_string()));
    }

    #[test]
    fn convert_install_with_force() {
        let cmd = CliCommand::Install {
            version: "1.22.4".to_string(),
            force: true,
            download: download_args(3),
        };
        let (name, args) = command_to_name_and_args(&cmd);
        assert_eq!(name, "install");
        assert_eq!(args, vec!["1.22.4".to_string(), "--force".to_string()]);
    }

    #[test]
    fn convert_use_default_local_uninstall() {
        assert_eq!(
            command_to_name_and_args(&CliCommand::Use {
                version: "1.22.4".to_string()
            }),
            ("use".to_string(), vec!["1.22.4".to_string()])
        );
        assert_eq!(
            command_to_name_and_args(&CliCommand::Default {
                version: "1.22.4".to_string()
            }),
            ("default".to_string(), vec!["1.22.4".to_string()])
        );
        assert_eq!(
            command_to_name_and_args(&CliCommand::Local {
                version: "1.22.4".to_string()
            }),
            ("local".to_string(), vec!["1.22.4".to_string()])
        );
        assert_eq!(
            command_to_name_and_args(&CliCommand::Uninstall {
                version: "1.22.4".to_string(),
                force: false,
            }),
            ("uninstall".to_string(), vec!["1.22.4".to_string()])
        );
        assert_eq!(
            command_to_name_and_args(&CliCommand::Uninstall {
                version: "1.22.4".to_string(),
                force: true,
            }),
            (
                "uninstall".to_string(),
                vec!["1.22.4".to_string(), "--force".to_string()]
            )
        );
    }

    #[test]
    fn convert_list_and_list_remote() {
        assert_eq!(
            command_to_name_and_args(&CliCommand::List { json: false }),
            ("list".to_string(), vec![])
        );
        assert_eq!(
            command_to_name_and_args(&CliCommand::List { json: true }),
            ("list".to_string(), vec!["--json".to_string()])
        );
        assert_eq!(
            command_to_name_and_args(&CliCommand::ListRemote {
                all: true,
                json: false
            }),
            ("list-remote".to_string(), vec!["--all".to_string()])
        );
        assert_eq!(
            command_to_name_and_args(&CliCommand::ListRemote {
                all: false,
                json: true
            }),
            ("list-remote".to_string(), vec!["--json".to_string()])
        );
        assert_eq!(
            command_to_name_and_args(&CliCommand::ListRemote {
                all: false,
                json: false
            }),
            ("list-remote".to_string(), vec![])
        );
    }

    #[test]
    fn convert_current_path_env() {
        assert_eq!(
            command_to_name_and_args(&CliCommand::Current),
            ("current".to_string(), vec![])
        );
        assert_eq!(
            command_to_name_and_args(&CliCommand::Path {
                version: Some("1.22.4".to_string())
            }),
            ("path".to_string(), vec!["1.22.4".to_string()])
        );
        assert_eq!(
            command_to_name_and_args(&CliCommand::Path { version: None }),
            ("path".to_string(), vec![])
        );
        assert_eq!(
            command_to_name_and_args(&CliCommand::Env {
                shell: Some("bash".to_string())
            }),
            ("env".to_string(), vec!["bash".to_string()])
        );
        assert_eq!(
            command_to_name_and_args(&CliCommand::Env { shell: None }),
            ("env".to_string(), vec![])
        );
    }

    #[test]
    fn convert_setup_with_shell_and_reset() {
        let cmd = CliCommand::Setup {
            shell: Some("zsh".to_string()),
            reset: true,
        };
        let (name, args) = command_to_name_and_args(&cmd);
        assert_eq!(name, "setup");
        assert_eq!(
            args,
            vec![
                "--shell".to_string(),
                "zsh".to_string(),
                "--reset".to_string()
            ]
        );
    }

    #[test]
    fn convert_exec_appends_trailing_args() {
        let cmd = CliCommand::Exec {
            version: "1.22.4".to_string(),
            args: vec!["build".to_string(), "./...".to_string()],
        };
        let (name, args) = command_to_name_and_args(&cmd);
        assert_eq!(name, "exec");
        assert_eq!(
            args,
            vec![
                "1.22.4".to_string(),
                "build".to_string(),
                "./...".to_string()
            ]
        );
    }

    #[test]
    fn convert_doctor_completions_upgrade_implode() {
        assert_eq!(
            command_to_name_and_args(&CliCommand::Doctor {
                shell: Some("fish".to_string()),
                fix: false
            }),
            (
                "doctor".to_string(),
                vec!["--shell".to_string(), "fish".to_string()]
            )
        );
        assert_eq!(
            command_to_name_and_args(&CliCommand::Doctor {
                shell: None,
                fix: true
            }),
            ("doctor".to_string(), vec!["--fix".to_string()])
        );
        assert_eq!(
            command_to_name_and_args(&CliCommand::Doctor {
                shell: None,
                fix: false
            }),
            ("doctor".to_string(), vec![])
        );
        assert_eq!(
            command_to_name_and_args(&CliCommand::Completions {
                shell: "bash".to_string()
            }),
            ("completions".to_string(), vec!["bash".to_string()])
        );
        assert_eq!(
            command_to_name_and_args(&CliCommand::Upgrade {
                force: true,
                download: download_args(3)
            }),
            ("upgrade".to_string(), vec!["--force".to_string()])
        );
        assert_eq!(
            command_to_name_and_args(&CliCommand::Implode { force: false }),
            ("implode".to_string(), vec![])
        );
    }

    #[test]
    fn convert_outdated_and_prune() {
        assert_eq!(
            command_to_name_and_args(&CliCommand::Outdated { json: false }),
            ("outdated".to_string(), vec![])
        );
        assert_eq!(
            command_to_name_and_args(&CliCommand::Outdated { json: true }),
            ("outdated".to_string(), vec!["--json".to_string()])
        );
        let cmd = CliCommand::Prune {
            force: true,
            dry_run: true,
            scan_dir: Some("/tmp/scan".to_string()),
        };
        let (name, args) = command_to_name_and_args(&cmd);
        assert_eq!(name, "prune");
        assert_eq!(
            args,
            vec![
                "--force".to_string(),
                "--dry-run".to_string(),
                "--scan-dir".to_string(),
                "/tmp/scan".to_string(),
            ]
        );
    }

    #[test]
    fn convert_shell_all_fields() {
        let cmd = CliCommand::Shell {
            version: Some("1.22.4".to_string()),
            unset: true,
            shell: Some("powershell".to_string()),
        };
        let (name, args) = command_to_name_and_args(&cmd);
        assert_eq!(name, "shell");
        assert_eq!(
            args,
            vec![
                "1.22.4".to_string(),
                "--unset".to_string(),
                "--shell".to_string(),
                "powershell".to_string(),
            ]
        );
    }

    #[test]
    fn convert_shell_no_optional_fields() {
        let cmd = CliCommand::Shell {
            version: None,
            unset: false,
            shell: None,
        };
        let (name, args) = command_to_name_and_args(&cmd);
        assert_eq!(name, "shell");
        assert!(args.is_empty());
    }
}
