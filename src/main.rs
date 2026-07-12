use std::{
    collections::BTreeSet,
    fs,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, anyhow, bail};
use clap::Parser;
use serde::{Deserialize, Serialize};
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};

wasmtime::component::bindgen!({ path: "wit", world: "module" });

const PROTOCOL_VERSION: u32 = 1;
const DEFAULT_FUEL: u64 = 20_000_000;
const MAX_HTTP_BODY: usize = 2 * 1024 * 1024;
const MAX_CACHE_VALUE: usize = 1024 * 1024;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    module: PathBuf,
    #[arg(long)]
    cache_dir: PathBuf,
    #[arg(long = "network-origin")]
    network_origins: Vec<String>,
    #[arg(long, default_value_t = 32)]
    memory_mib: usize,
    #[arg(long, default_value_t = DEFAULT_FUEL)]
    fuel: u64,
}

struct HostState {
    limits: StoreLimits,
    cache_dir: PathBuf,
    network_origins: BTreeSet<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Request {
    Handshake {
        protocol: u32,
    },
    Query {
        id: u64,
        query: String,
        max_results: u32,
        locale: Option<String>,
        #[serde(default = "default_settings_json")]
        settings_json: String,
    },
}

#[derive(Serialize)]
struct Response<'a, T: Serialize> {
    #[serde(rename = "type")]
    kind: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct QueryValue {
    results: Vec<ResultValue>,
    exclusive: bool,
}

#[derive(Serialize)]
struct ResultValue {
    id: String,
    title: String,
    subtitle: String,
    icon: IconValue,
    score: Option<u32>,
    action: ActionValue,
}

#[derive(Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum IconValue {
    PackagePath(String),
    Text(String),
    None,
}

#[derive(Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum ActionValue {
    CopyText(String),
    OpenUrl(String),
    OpenPath(String),
    ShowMessage(String),
    Notify((String, String)),
    RunApprovedCommand(Vec<String>),
    ScheduleNotification((u64, String, String)),
    ScheduleCommand((u64, Vec<String>)),
    None,
}

impl wasmtime::ResourceLimiter for HostState {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> std::result::Result<bool, wasmtime::Error> {
        self.limits.memory_growing(current, desired, maximum)
    }
    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> std::result::Result<bool, wasmtime::Error> {
        self.limits.table_growing(current, desired, maximum)
    }
}

impl rayslash::module::host::Host for HostState {
    fn unix_time(&mut self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn request(
        &mut self,
        request: rayslash::module::host::HttpRequest,
    ) -> std::result::Result<
        rayslash::module::host::HttpResponse,
        rayslash::module::types::ModuleError,
    > {
        self.http_request(request)
    }

    fn cache_get(
        &mut self,
        key: String,
    ) -> std::result::Result<Option<Vec<u8>>, rayslash::module::types::ModuleError> {
        if !valid_cache_key(&key) {
            return Err(module_error("invalid cache key"));
        }
        match fs::read(self.cache_dir.join(key)) {
            Ok(value) if value.len() <= MAX_CACHE_VALUE => Ok(Some(value)),
            Ok(_) => Err(module_error("cached value exceeds limit")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(module_error("cache read failed")),
        }
    }

    fn cache_put(
        &mut self,
        key: String,
        value: Vec<u8>,
    ) -> std::result::Result<(), rayslash::module::types::ModuleError> {
        if !valid_cache_key(&key) || value.len() > MAX_CACHE_VALUE {
            return Err(module_error("invalid cache write"));
        }
        if fs::create_dir_all(&self.cache_dir).is_err() {
            return Err(module_error("cache directory unavailable"));
        }
        let target = self.cache_dir.join(&key);
        let temporary = self.cache_dir.join(format!(".{key}.tmp"));
        if fs::write(&temporary, value)
            .and_then(|_| fs::rename(temporary, target))
            .is_err()
        {
            return Err(module_error("cache write failed"));
        }
        Ok(())
    }
}

impl rayslash::module::types::Host for HostState {}

impl HostState {
    fn http_request(
        &self,
        request: rayslash::module::host::HttpRequest,
    ) -> Result<rayslash::module::host::HttpResponse, rayslash::module::types::ModuleError> {
        if request.body.len() > MAX_HTTP_BODY || request.headers.len() > 32 {
            return Err(module_error("HTTP request exceeds limits"));
        }
        let Some(origin) = https_origin(&request.url) else {
            return Err(module_error("only absolute HTTPS URLs are allowed"));
        };
        if !self.network_origins.contains(origin) {
            return Err(module_error("network origin was not granted"));
        }
        let method = request.method.to_ascii_uppercase();
        if !matches!(method.as_str(), "GET" | "POST") {
            return Err(module_error("HTTP method was not granted"));
        }
        let mut builder = ureq::http::Request::builder()
            .method(method.as_str())
            .uri(&request.url);
        for (name, value) in request.headers {
            let lower = name.to_ascii_lowercase();
            if matches!(
                lower.as_str(),
                "authorization" | "cookie" | "proxy-authorization" | "host" | "connection"
            ) {
                return Err(module_error("forbidden HTTP header"));
            }
            builder = builder.header(name, value);
        }
        let built = builder
            .header("User-Agent", "rayslash-module-host/1")
            .body(request.body)
            .map_err(|_| module_error("invalid HTTP request"))?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .build()
            .into();
        let mut response = agent
            .run(built)
            .map_err(|_| module_error("HTTP request failed"))?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.to_string(), value.to_owned()))
            })
            .take(64)
            .collect();
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_HTTP_BODY as u64)
            .read_to_vec()
            .map_err(|_| module_error("HTTP response exceeds limits"))?;
        Ok(rayslash::module::host::HttpResponse {
            status,
            headers,
            body,
        })
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rayslash-module-host: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    if args.memory_mib == 0 || args.memory_mib > 256 {
        bail!("memory limit must be between 1 and 256 MiB");
    }
    for origin in &args.network_origins {
        if https_origin(origin).is_none_or(|parsed| parsed != origin) {
            bail!("network origin must be an exact HTTPS origin: {origin}");
        }
    }
    let mut config = Config::new();
    config.wasm_component_model(true).consume_fuel(true);
    let engine = wt(Engine::new(&config))?;
    let component = wt(Component::from_file(&engine, &args.module))
        .map_err(|error| anyhow!("failed to load module component: {error}"))?;
    let mut linker = Linker::new(&engine);
    wt(Module::add_to_linker::<
        HostState,
        wasmtime::component::HasSelf<HostState>,
    >(&mut linker, |state| state))?;
    let limits = StoreLimitsBuilder::new()
        .memory_size(args.memory_mib * 1024 * 1024)
        .instances(2)
        .tables(8)
        .build();
    let mut store = Store::new(
        &engine,
        HostState {
            limits,
            cache_dir: args.cache_dir,
            network_origins: args.network_origins.into_iter().collect(),
        },
    );
    store.limiter(|state| state);
    wt(store.set_fuel(args.fuel))?;
    let bindings = wt(Module::instantiate(&mut store, &component, &linker))?;

    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    for line in stdin.lock().lines() {
        let line = line?;
        if line.len() > 256 * 1024 {
            write_error(&mut stdout, None, "request exceeds limit")?;
            continue;
        }
        match serde_json::from_str::<Request>(&line) {
            Ok(Request::Handshake { protocol }) if protocol == PROTOCOL_VERSION => write_response(
                &mut stdout,
                &Response {
                    kind: "handshake",
                    id: None,
                    value: Some(PROTOCOL_VERSION),
                    error: None,
                },
            )?,
            Ok(Request::Handshake { .. }) => {
                write_error(&mut stdout, None, "unsupported protocol")?
            }
            Ok(Request::Query {
                id,
                query,
                max_results,
                locale,
                settings_json,
            }) => {
                if query.chars().count() > 4096
                    || settings_json.len() > 256 * 1024
                    || max_results == 0
                    || max_results > 100
                {
                    write_error(&mut stdout, Some(id), "invalid query limits")?;
                    continue;
                }
                wt(store.set_fuel(args.fuel))?;
                let context = rayslash::module::types::QueryContext {
                    query,
                    max_results,
                    locale,
                    settings_json,
                };
                match bindings
                    .rayslash_module_provider()
                    .call_query(&mut store, &context)
                {
                    Ok(Ok(value)) => {
                        let results = value
                            .results
                            .into_iter()
                            .take(max_results as usize)
                            .map(map_result)
                            .collect::<Result<Vec<_>>>()?;
                        let value = QueryValue {
                            results,
                            exclusive: value.exclusive,
                        };
                        write_response(
                            &mut stdout,
                            &Response {
                                kind: "query",
                                id: Some(id),
                                value: Some(value),
                                error: None,
                            },
                        )?;
                    }
                    Ok(Err(error)) => write_error(&mut stdout, Some(id), &format!("{error:?}"))?,
                    Err(error) => {
                        write_error(&mut stdout, Some(id), &format!("module trapped: {error}"))?
                    }
                }
            }
            Err(_) => write_error(&mut stdout, None, "invalid request")?,
        }
    }
    Ok(())
}

fn map_result(value: rayslash::module::types::ResultItem) -> Result<ResultValue> {
    use rayslash::module::types::{Action, Icon};
    valid_text("result ID", &value.id, 1, 256)?;
    valid_text("result title", &value.title, 1, 512)?;
    valid_text("result subtitle", &value.subtitle, 0, 1024)?;
    let icon = match value.icon {
        Icon::PackagePath(value) => {
            let path = Path::new(&value);
            if value.len() > 512
                || value.is_empty()
                || path.is_absolute()
                || path
                    .components()
                    .any(|part| !matches!(part, std::path::Component::Normal(_)))
            {
                bail!("invalid package icon path");
            }
            IconValue::PackagePath(value)
        }
        Icon::Text(value) => {
            valid_text("text icon", &value, 1, 16)?;
            IconValue::Text(value)
        }
        Icon::None => IconValue::None,
    };
    let action = match value.action {
        Action::CopyText(value) => {
            valid_text("copy action", &value, 0, 64 * 1024)?;
            ActionValue::CopyText(value)
        }
        Action::OpenUrl(value) => {
            if value.len() > 4096 || https_origin(&value).is_none() {
                bail!("invalid open URL action");
            }
            ActionValue::OpenUrl(value)
        }
        Action::OpenPath(value) => {
            valid_text("open path action", &value, 1, 4096)?;
            ActionValue::OpenPath(value)
        }
        Action::ShowMessage(value) => {
            valid_text("message action", &value, 0, 4096)?;
            ActionValue::ShowMessage(value)
        }
        Action::Notify((title, body)) => {
            valid_text("notification title", &title, 1, 256)?;
            valid_text("notification body", &body, 0, 4096)?;
            ActionValue::Notify((title, body))
        }
        Action::RunApprovedCommand(value) => {
            validate_command(&value)?;
            ActionValue::RunApprovedCommand(value)
        }
        Action::ScheduleNotification((delay, title, body)) => {
            validate_delay(delay)?;
            valid_text("notification title", &title, 1, 256)?;
            valid_text("notification body", &body, 0, 4096)?;
            ActionValue::ScheduleNotification((delay, title, body))
        }
        Action::ScheduleCommand((delay, command)) => {
            validate_delay(delay)?;
            validate_command(&command)?;
            ActionValue::ScheduleCommand((delay, command))
        }
        Action::None => ActionValue::None,
    };
    Ok(ResultValue {
        id: value.id,
        title: value.title,
        subtitle: value.subtitle,
        score: value.score,
        icon,
        action,
    })
}

fn valid_text(label: &str, value: &str, minimum: usize, maximum: usize) -> Result<()> {
    let length = value.chars().count();
    if length < minimum || length > maximum || value.chars().any(char::is_control) {
        bail!("invalid {label}");
    }
    Ok(())
}

fn validate_command(command: &[String]) -> Result<()> {
    if command.is_empty() || command.len() > 32 {
        bail!("invalid command action");
    }
    for (index, argument) in command.iter().enumerate() {
        valid_text("command argument", argument, usize::from(index == 0), 4096)?;
    }
    Ok(())
}

fn validate_delay(delay: u64) -> Result<()> {
    if delay > 31_536_000 {
        bail!("scheduled action delay exceeds one year");
    }
    Ok(())
}

fn module_error(message: &str) -> rayslash::module::types::ModuleError {
    rayslash::module::types::ModuleError::Unavailable(message.to_owned())
}
fn valid_cache_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}
fn default_settings_json() -> String {
    "{}".to_owned()
}
fn https_origin(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let end = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..end];
    (!authority.is_empty() && !authority.contains(['@', '?', '#']))
        .then(|| &url[.."https://".len() + end])
}
fn write_response(output: &mut impl Write, response: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *output, response)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}
fn write_error(output: &mut impl Write, id: Option<u64>, error: &str) -> Result<()> {
    write_response(
        output,
        &Response::<()> {
            kind: "error",
            id,
            value: None,
            error: Some(error.to_owned()),
        },
    )
}

fn wt<T>(result: std::result::Result<T, wasmtime::Error>) -> Result<T> {
    result.map_err(|error| anyhow!(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn origins_are_exact() {
        assert_eq!(
            https_origin("https://example.com/a"),
            Some("https://example.com")
        );
        assert_eq!(https_origin("http://example.com"), None);
    }
    #[test]
    fn cache_keys_reject_paths() {
        assert!(valid_cache_key("rates-v1.json"));
        assert!(!valid_cache_key("../rates"));
        assert!(!valid_cache_key("a/b"));
    }
    #[test]
    fn guest_text_and_actions_are_bounded() {
        assert!(valid_text("title", "Result", 1, 512).is_ok());
        assert!(valid_text("title", "bad\nvalue", 1, 512).is_err());
        assert!(valid_text("title", &"x".repeat(513), 1, 512).is_err());
        assert!(validate_command(&["systemctl".into(), "reboot".into()]).is_ok());
        assert!(validate_command(&[]).is_err());
        assert!(validate_delay(31_536_000).is_ok());
        assert!(validate_delay(31_536_001).is_err());
    }
}
