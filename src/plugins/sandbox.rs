//! Sandbox — WASM-isolated execution with fuel.

use std::sync::Arc;

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::{json, Value};
use wasmtime::{Caller, Config, Engine, Linker, Module, Store};

use crate::capability::{AnyCapability, Capability, CapabilityService, SyncKind, SyncResource};

const DEFAULT_FUEL: u64 = 1_000_000;

pub struct SandboxState {
    pub stdout: String,
}

pub struct SandboxResource;

impl SyncResource for SandboxResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let path = input
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "sandbox.exec: input.path must be a string".to_string())?;
        let fuel = input
            .get("fuel")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_FUEL);

        let wat_text = std::fs::read_to_string(path)
            .map_err(|e| format!("sandbox: read {path}: {e}"))?;
        let wasm_bytes =
            wat::parse_str(&wat_text).map_err(|e| format!("sandbox: parse WAT: {e}"))?;

        let mut config = Config::new();
        config.consume_fuel(true);
        let engine = Engine::new(&config).map_err(|e| format!("sandbox: engine: {e}"))?;
        let module =
            Module::new(&engine, &wasm_bytes).map_err(|e| format!("sandbox: module: {e}"))?;

        let mut store = Store::new(&engine, SandboxState { stdout: String::new() });
        store.set_fuel(fuel).map_err(|e| format!("sandbox: fuel: {e}"))?;

        let mut linker = Linker::new(&engine);
        linker
            .func_wrap(
                "host",
                "log",
                |mut caller: Caller<'_, SandboxState>, ptr: i32, len: i32| {
                    let Some(mem) = caller.get_export("memory").and_then(|e| e.into_memory()) else {
                        return;
                    };
                    let data = mem.data(&caller);
                    let start = ptr as usize;
                    let end = (ptr + len) as usize;
                    if end <= data.len() {
                        let s = String::from_utf8_lossy(&data[start..end]).to_string();
                        caller.data_mut().stdout.push_str(&s);
                    }
                },
            )
            .map_err(|e| format!("sandbox: linker: {e}"))?;

        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| format!("sandbox: instantiate: {e}"))?;
        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .map_err(|e| format!("sandbox: get run: {e}"))?;

        let exit_code = match run.call(&mut store, ()) {
            Ok(code) => code,
            Err(e) => {
                let fuel_used = fuel.saturating_sub(store.get_fuel().unwrap_or(0));
                let stdout = store.into_data().stdout;
                return Ok(json!({
                    "exit_code": -1,
                    "fuel_used": fuel_used,
                    "stdout": stdout,
                    "error": format!("{e}"),
                }));
            }
        };

        let fuel_used = fuel.saturating_sub(store.get_fuel().unwrap_or(0));
        let stdout = store.into_data().stdout;
        Ok(json!({
            "exit_code": exit_code,
            "fuel_used": fuel_used,
            "stdout": stdout,
        }))
    }
}

pub fn handler() -> Arc<SandboxResource> {
    Arc::new(SandboxResource)
}

pub fn sandbox_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "sandbox",
        vec![
            Injection::from("cap:exec"),
            Injection::from("capability_service"),
        ],
        |ctx: Context, _cfg: ()| async move {
            let exec_cap: Arc<Capability<SandboxResource, SyncKind>> = ctx.require("cap:exec")?;
            let cap_svc: Arc<CapabilityService> = ctx.require("capability_service")?;
            ctx.logger().log(
                LogLevel::Info,
                "sandbox plugin: WASM execution capability activated".into(),
            );
            cap_svc.register(exec_cap as Arc<dyn AnyCapability>)?;
            Ok(())
        },
    )
}