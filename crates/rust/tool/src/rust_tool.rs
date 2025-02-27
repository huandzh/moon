use moon_config::RustConfig;
use moon_logger::debug;
use moon_platform_runtime::RuntimeReq;
use moon_process::Command;
use moon_terminal::{print_checkpoint, Checkpoint};
use moon_tool::{
    async_trait, get_proto_paths, load_tool_plugin, prepend_path_env_var, use_global_tool_on_path,
    Tool,
};
use proto_core::{Id, ProtoEnvironment, Tool as ProtoTool, UnresolvedVersionSpec};
use rustc_hash::FxHashMap;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::{ffi::OsStr, path::Path};

pub fn get_rust_env_paths(proto_env: &ProtoEnvironment) -> Vec<PathBuf> {
    let mut paths = get_proto_paths(proto_env);

    if let Ok(value) = env::var("CARGO_INSTALL_ROOT") {
        paths.push(PathBuf::from(value).join("bin"));
    }

    if let Ok(value) = env::var("CARGO_HOME") {
        paths.push(PathBuf::from(value).join("bin"));
    }

    paths.push(proto_env.home.join(".cargo").join("bin"));

    paths
}

pub struct RustTool {
    pub config: RustConfig,

    pub global: bool,

    pub tool: ProtoTool,

    proto_env: Arc<ProtoEnvironment>,
}

impl RustTool {
    pub async fn new(
        proto_env: Arc<ProtoEnvironment>,
        config: &RustConfig,
        req: &RuntimeReq,
    ) -> miette::Result<RustTool> {
        let mut rust = RustTool {
            config: config.to_owned(),
            global: false,
            tool: load_tool_plugin(
                &Id::raw("rust"),
                &proto_env,
                config.plugin.as_ref().unwrap(),
            )
            .await?,
            proto_env,
        };

        if use_global_tool_on_path() || req.is_global() {
            rust.global = true;
            rust.config.version = None;
        } else {
            rust.config.version = req.to_spec();
        };

        Ok(rust)
    }

    pub async fn exec_cargo<I, S>(&self, args: I, working_dir: &Path) -> miette::Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        Command::new("cargo")
            .args(args)
            .env(
                "PATH",
                prepend_path_env_var(get_rust_env_paths(&self.proto_env)),
            )
            .cwd(working_dir)
            .create_async()
            .exec_stream_output()
            .await?;

        Ok(())
    }

    pub async fn exec_rustup<I, S>(&self, args: I, working_dir: &Path) -> miette::Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        Command::new("rustup")
            .args(args)
            .env(
                "PATH",
                prepend_path_env_var(get_rust_env_paths(&self.proto_env)),
            )
            .cwd(working_dir)
            .create_async()
            .exec_stream_output()
            .await?;

        Ok(())
    }
}

#[async_trait]
impl Tool for RustTool {
    fn as_any(&self) -> &(dyn std::any::Any + Send + Sync) {
        self
    }

    async fn setup(
        &mut self,
        last_versions: &mut FxHashMap<String, UnresolvedVersionSpec>,
    ) -> miette::Result<u8> {
        let mut installed = 0;

        let Some(version) = &self.config.version else {
            return Ok(installed);
        };

        if self.global {
            debug!("Using global binary in PATH");
        } else if self.tool.is_setup(version).await? {
            debug!("Rust has already been setup");

            // When offline and the tool doesn't exist, fallback to the global binary
        } else if proto_core::is_offline() {
            debug!(
                "No internet connection and Rust has not been setup, falling back to global binary in PATH"
            );

            self.global = true;

            // Otherwise try and install the tool
        } else {
            let setup = match last_versions.get("rust") {
                Some(last) => version != last,
                None => true,
            };

            if setup || !self.tool.get_tool_dir().exists() {
                print_checkpoint(format!("installing rust {version}"), Checkpoint::Setup);

                if self.tool.setup(version, false).await? {
                    last_versions.insert("rust".into(), version.to_owned());
                    installed += 1;
                }
            }
        }

        self.tool.locate_globals_dir().await?;

        Ok(installed)
    }

    async fn teardown(&mut self) -> miette::Result<()> {
        self.tool.teardown().await?;

        Ok(())
    }
}
