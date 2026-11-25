//! A debug adapter, `FixtureDebug`: tools/e2e/dap_fixture.py, given as the adapter's binary
//! (`dap.FixtureDebug.binary`), run with python3. And a debug locator turning a task that runs
//! `fixture-run <program>` into a launch of the program with that adapter.

use zed_extension_api::{
    self as zed, DebugAdapterBinary, DebugConfig, DebugRequest, DebugScenario, DebugTaskDefinition,
    LaunchRequest, Result, StartDebuggingRequestArguments, StartDebuggingRequestArgumentsRequest,
    TaskTemplate,
    serde_json::{self, Value, json},
};

struct FixtureDebug;

impl zed::Extension for FixtureDebug {
    fn new() -> Self {
        FixtureDebug
    }

    fn get_dap_binary(
        &mut self,
        adapter_name: String,
        config: DebugTaskDefinition,
        user_provided_debug_adapter_path: Option<String>,
        worktree: &zed::Worktree,
    ) -> Result<DebugAdapterBinary, String> {
        let script = user_provided_debug_adapter_path
            .ok_or("set dap.FixtureDebug.binary to the adapter's script")?;
        let python = worktree
            .which("python3")
            .ok_or("python3 is not on the PATH")?;
        let configuration: Value =
            serde_json::from_str(&config.config).map_err(|error| error.to_string())?;
        let request = self.dap_request_kind(adapter_name, configuration)?;
        Ok(DebugAdapterBinary {
            command: Some(python),
            arguments: vec![script],
            envs: Vec::new(),
            cwd: None,
            connection: None,
            request_args: StartDebuggingRequestArguments {
                configuration: config.config,
                request,
            },
        })
    }

    fn dap_request_kind(
        &mut self,
        _adapter_name: String,
        config: Value,
    ) -> Result<StartDebuggingRequestArgumentsRequest, String> {
        match config.get("request").and_then(Value::as_str) {
            Some("launch") => Ok(StartDebuggingRequestArgumentsRequest::Launch),
            Some("attach") => Ok(StartDebuggingRequestArgumentsRequest::Attach),
            _ => Err("\"request\" must be \"launch\" or \"attach\"".into()),
        }
    }

    fn dap_config_to_scenario(&mut self, config: DebugConfig) -> Result<DebugScenario, String> {
        let configuration = match config.request {
            DebugRequest::Launch(launch) => launch_configuration(&launch),
            DebugRequest::Attach(attach) => json!({"request": "attach", "pid": attach.process_id}),
        };
        Ok(DebugScenario {
            label: config.label,
            adapter: config.adapter,
            build: None,
            config: configuration.to_string(),
            tcp_connection: None,
        })
    }

    fn dap_locator_create_scenario(
        &mut self,
        _locator_name: String,
        build_task: TaskTemplate,
        resolved_label: String,
        debug_adapter_name: String,
    ) -> Option<DebugScenario> {
        if build_task.command != "fixture-run" {
            return None;
        }
        let launch = LaunchRequest {
            program: build_task.args.first()?.clone(),
            cwd: build_task.cwd,
            args: build_task.args.into_iter().skip(1).collect(),
            envs: build_task.env,
        };
        Some(DebugScenario {
            label: resolved_label,
            adapter: debug_adapter_name,
            build: None,
            config: launch_configuration(&launch).to_string(),
            tcp_connection: None,
        })
    }
}

fn launch_configuration(launch: &LaunchRequest) -> Value {
    json!({
        "request": "launch",
        "program": launch.program,
        "cwd": launch.cwd,
        "args": launch.args,
    })
}

zed::register_extension!(FixtureDebug);
