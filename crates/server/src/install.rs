//! Password-authenticated SSH onboarding.
//!
//! The Web UI sends the target user / host / password once. The server logs in
//! over SSH, uploads the agent next to the server binary, writes its config and
//! starts it (systemd --user when available, nohup otherwise). The password is
//! used only for this request and is never logged or stored.

use crate::AppState;
use axum::{Json, extract::State, http::StatusCode};
use common::config::AgentConfig;
use serde::{Deserialize, Serialize};
use ssh2::{ExtendedData, Session};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const SSH_CONNECT_TIMEOUT: Duration = Duration::from_secs(12);
const SSH_SESSION_TIMEOUT: Duration = Duration::from_secs(90);
const DEFAULT_SSH_PORT: u16 = 22;

type ApiError = (StatusCode, Json<serde_json::Value>);
type ApiResult<T> = Result<T, ApiError>;

#[derive(Deserialize)]
pub struct InstallRequest {
    pub username: String,
    pub host: String,
    pub password: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    #[serde(default)]
    pub node_id: String,
    #[serde(default)]
    pub server_addr: String,
}

#[derive(Serialize)]
pub struct InstallResponse {
    pub node_id: String,
    pub message: String,
}

struct ValidatedRequest {
    username: String,
    host: String,
    port: u16,
    password: String,
    node_id: String,
    server_addr: String,
}

fn default_ssh_port() -> u16 {
    DEFAULT_SSH_PORT
}

fn fail(status: StatusCode, message: impl Into<String>) -> ApiError {
    (status, Json(serde_json::json!({ "error": message.into() })))
}

pub async fn install_agent(
    State(state): State<Arc<AppState>>,
    Json(request): Json<InstallRequest>,
) -> ApiResult<Json<InstallResponse>> {
    let request = validate(request)?;
    let agent_path = agent_binary_path()
        .map_err(|message| fail(StatusCode::INTERNAL_SERVER_ERROR, message))?;
    let bytes = std::fs::read(&agent_path).map_err(|error| {
        fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("读取 Agent 二进制失败：{error}"),
        )
    })?;
    let remote_name = "clusterscope";
    let role_args = "agent";
    let agent_token = state.config.agent_token.clone();

    let result = tokio::task::spawn_blocking(move || {
        install_blocking(
            &request,
            &bytes,
            remote_name,
            role_args,
            &agent_token,
        )
    })
    .await
    .map_err(|_| fail(StatusCode::INTERNAL_SERVER_ERROR, "安装任务异常退出。"))??;

    Ok(Json(result))
}

fn validate(request: InstallRequest) -> ApiResult<ValidatedRequest> {
    let username = request.username.trim().to_string();
    if username.is_empty() {
        return Err(fail(StatusCode::BAD_REQUEST, "SSH 用户名不能为空。"));
    }
    if username.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "SSH 用户名不能包含空格或控制字符。",
        ));
    }

    let host = request.host.trim().to_string();
    if host.is_empty() {
        return Err(fail(StatusCode::BAD_REQUEST, "目标 IP / 主机名不能为空。"));
    }
    if host.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "目标 IP / 主机名不能包含空格或控制字符。",
        ));
    }

    if request.password.is_empty() {
        return Err(fail(StatusCode::BAD_REQUEST, "SSH 密码不能为空。"));
    }

    let port = if request.port == 0 { DEFAULT_SSH_PORT } else { request.port };

    let node_id = request.node_id.trim().to_string();
    if node_id.len() > 255 || node_id.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "节点 ID 最长 255 个字符，且不能包含空格或控制字符。",
        ));
    }

    let server_addr = normalize_server_addr(&request.server_addr)?;

    Ok(ValidatedRequest {
        username,
        host,
        port,
        password: request.password,
        node_id,
        server_addr,
    })
}

fn normalize_server_addr(raw: &str) -> ApiResult<String> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(fail(StatusCode::BAD_REQUEST, "Server gRPC 地址不能为空。"));
    }
    if value.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "Server gRPC 地址不能包含空格或控制字符。",
        ));
    }
    let with_scheme = if value.starts_with("http://") || value.starts_with("https://") {
        value.to_string()
    } else {
        format!("http://{value}")
    };
    let _: axum::http::Uri = with_scheme
        .parse()
        .map_err(|_| fail(StatusCode::BAD_REQUEST, "Server gRPC 地址格式不正确。"))?;
    Ok(with_scheme)
}

fn agent_binary_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|error| format!("无法定位 Server 可执行文件：{error}"))?;
    if exe.file_name().and_then(|name| name.to_str()) != Some("clusterscope") {
        return Err("当前进程不是 clusterscope；请用单二进制 clusterscope server 启动 Server。".to_string());
    }
    Ok(exe)
}

fn install_blocking(
    request: &ValidatedRequest,
    agent_bytes: &[u8],
    remote_name: &str,
    role_args: &str,
    agent_token: &str,
) -> ApiResult<InstallResponse> {
    let mut addresses = (request.host.as_str(), request.port)
        .to_socket_addrs()
        .map_err(|error| {
            fail(
                StatusCode::BAD_GATEWAY,
                format!("无法解析目标主机（{}）：{error}", request.host),
            )
        })?;
    let address = addresses.next().ok_or_else(|| {
        fail(
            StatusCode::BAD_GATEWAY,
            format!("无法解析目标主机（{}）。", request.host),
        )
    })?;

    let tcp = TcpStream::connect_timeout(&address, SSH_CONNECT_TIMEOUT).map_err(|error| {
        fail(
            StatusCode::BAD_GATEWAY,
            format!(
                "无法连接目标主机的 SSH（{}:{}）：{error}",
                request.host, request.port
            ),
        )
    })?;
    tcp.set_read_timeout(Some(SSH_SESSION_TIMEOUT)).ok();
    tcp.set_write_timeout(Some(SSH_SESSION_TIMEOUT)).ok();

    let mut session = Session::new()
        .map_err(|_| fail(StatusCode::INTERNAL_SERVER_ERROR, "无法初始化 SSH 客户端。"))?;
    session.set_tcp_stream(tcp);
    session.set_timeout(SSH_SESSION_TIMEOUT.as_millis() as u32);
    session.handshake().map_err(|error| {
        fail(
            StatusCode::BAD_GATEWAY,
            format!("SSH 握手失败（{}）：{error}", request.host),
        )
    })?;
    session
        .userauth_password(&request.username, &request.password)
        .map_err(|_| {
            fail(
                StatusCode::BAD_GATEWAY,
                "SSH 登录失败，请检查用户名、IP 和密码。",
            )
        })?;
    if !session.authenticated() {
        return Err(fail(
            StatusCode::BAD_GATEWAY,
            "SSH 登录失败，请检查用户名、IP 和密码。",
        ));
    }

    let home = run(&session, "printf %s \"$HOME\"")?;
    let home = home.trim();
    if !home.starts_with('/') || home.contains('\n') {
        return Err(fail(
            StatusCode::BAD_GATEWAY,
            "目标主机的 HOME 路径异常，已停止安装。",
        ));
    }

    let arch = run(&session, "uname -m")?;
    let arch = arch.trim();
    if arch != "x86_64" {
        return Err(fail(
            StatusCode::BAD_GATEWAY,
            format!("目标主机架构是 {arch}，当前 Agent 只提供 x86_64。"),
        ));
    }

    let remote_hostname = run(&session, "hostname")?;
    let remote_hostname = remote_hostname.trim();
    if remote_hostname.is_empty() {
        return Err(fail(
            StatusCode::BAD_GATEWAY,
            "无法读取目标主机名，已停止安装。",
        ));
    }
    let node_id = if request.node_id.is_empty() {
        remote_hostname.to_string()
    } else {
        request.node_id.clone()
    };

    let bin_dir = format!("{home}/.local/bin");
    let config_dir = format!("{home}/.config/clusterscope");
    let log_dir = format!("{config_dir}/logs");
    let bin_path = format!("{bin_dir}/{remote_name}");
    let tmp_path = format!("{bin_path}.new");
    let config_path = format!("{config_dir}/agent.yaml");
    let pid_path = format!("{config_dir}/agent.pid");

    run(
        &session,
        &format!(
            "mkdir -p {} {} {}",
            shell_quote(&bin_dir),
            shell_quote(&config_dir),
            shell_quote(&log_dir)
        ),
    )?;

    upload(&session, &tmp_path, agent_bytes)?;
    run(
        &session,
        &format!(
            "mv -f {tmp} {bin} && chmod 755 {bin}",
            tmp = shell_quote(&tmp_path),
            bin = shell_quote(&bin_path)
        ),
    )?;

    let mut config = AgentConfig::default();
    config.server_addr = request.server_addr.clone();
    config.node_id = Some(node_id.clone());
    config.node_id_file = PathBuf::from(format!("{config_dir}/node_id"));
    config.log_dir = PathBuf::from(&log_dir);
    config.report_interval_secs = 2;
    config.log_level = "info".to_string();
    config.disk_mounts = vec!["/".to_string()];
    config.collect_process_details = true;
    config.agent_token = agent_token.to_string();
    let yaml = serde_yaml::to_string(&config)
        .map_err(|error| fail(StatusCode::INTERNAL_SERVER_ERROR, format!("生成 Agent 配置失败：{error}")))?;
    upload(&session, &config_path, yaml.as_bytes())?;

    let start_script = format!(
        r#"set -eu
BIN={bin}
CONF={conf}
PIDFILE={pid}

stop_previous_agent() {{
    [ -f "$PIDFILE" ] || return 0
    pid="$(cat "$PIDFILE" 2>/dev/null || true)"
    rm -f "$PIDFILE"
    case "$pid" in
        '' | *[!0-9]*) return 0 ;;
    esac
    [ -r "/proc/$pid/cmdline" ] || return 0
    cmdline="$(tr '\0' ' ' < "/proc/$pid/cmdline" || true)"
    case "$cmdline" in
        *"$BIN"*"$CONF"*) kill "$pid" 2>/dev/null || true ;;
    esac
}}

if command -v systemctl >/dev/null 2>&1 && systemctl --user show-environment >/dev/null 2>&1; then
    stop_previous_agent
    mkdir -p "$HOME/.config/systemd/user"
    cat > "$HOME/.config/systemd/user/clusterscope.service" <<SVC
[Unit]
Description=ClusterScope GPU Agent
After=network-online.target

[Service]
Type=simple
ExecStart="$BIN" {role_args} -c "$CONF"
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
SVC
    systemctl --user daemon-reload
    systemctl --user enable clusterscope.service
    systemctl --user restart clusterscope.service
    echo systemd
else
    stop_previous_agent
    mkdir -p "$HOME/.config/clusterscope/logs"
    nohup "$BIN" {role_args} -c "$CONF" >> "$HOME/.config/clusterscope/logs/agent.log" 2>&1 < /dev/null &
    echo $! > "$PIDFILE"
    echo nohup
fi
"#,
        bin = shell_quote(&bin_path),
        conf = shell_quote(&config_path),
        pid = shell_quote(&pid_path),
        role_args = role_args,
    );

    let started = exec_script(&session, &start_script)?;
    let started = started.trim();
    let via = if started.lines().any(|line| line.trim() == "systemd") {
        "systemd --user"
    } else {
        "nohup"
    };

    Ok(InstallResponse {
        node_id,
        message: format!("Agent 已安装并通过 {via} 启动。"),
    })
}

fn run(session: &Session, command: &str) -> ApiResult<String> {
    let mut channel = session
        .channel_session()
        .map_err(|_| fail(StatusCode::BAD_GATEWAY, "无法打开 SSH 命令通道。"))?;
    channel
        .handle_extended_data(ExtendedData::Merge)
        .map_err(|_| fail(StatusCode::BAD_GATEWAY, "无法读取 SSH 命令输出。"))?;
    channel
        .exec(command)
        .map_err(|error| fail(StatusCode::BAD_GATEWAY, format!("远程命令执行失败：{error}")))?;
    let mut output = String::new();
    channel
        .read_to_string(&mut output)
        .map_err(|_| fail(StatusCode::BAD_GATEWAY, "读取远程命令输出失败。"))?;
    channel
        .wait_close()
        .map_err(|_| fail(StatusCode::BAD_GATEWAY, "等待远程命令结束失败。"))?;
    let status = channel
        .exit_status()
        .map_err(|_| fail(StatusCode::BAD_GATEWAY, "读取远程命令状态失败。"))?;
    if status != 0 {
        let detail = output.trim();
        return Err(fail(
            StatusCode::BAD_GATEWAY,
            format!("远程命令失败（{status}）：{detail}"),
        ));
    }
    Ok(output)
}

fn exec_script(session: &Session, script: &str) -> ApiResult<String> {
    let mut channel = session
        .channel_session()
        .map_err(|_| fail(StatusCode::BAD_GATEWAY, "无法打开 SSH 脚本通道。"))?;
    channel
        .handle_extended_data(ExtendedData::Merge)
        .map_err(|_| fail(StatusCode::BAD_GATEWAY, "无法读取 SSH 脚本输出。"))?;
    channel
        .exec("bash -s")
        .map_err(|_| fail(StatusCode::BAD_GATEWAY, "无法启动远程安装脚本。"))?;
    channel
        .write_all(script.as_bytes())
        .map_err(|_| fail(StatusCode::BAD_GATEWAY, "发送远程安装脚本失败。"))?;
    channel
        .send_eof()
        .map_err(|_| fail(StatusCode::BAD_GATEWAY, "结束远程安装脚本失败。"))?;
    let mut output = String::new();
    channel
        .read_to_string(&mut output)
        .map_err(|_| fail(StatusCode::BAD_GATEWAY, "读取远程安装结果失败。"))?;
    channel
        .wait_close()
        .map_err(|_| fail(StatusCode::BAD_GATEWAY, "等待远程安装结束失败。"))?;
    let status = channel
        .exit_status()
        .map_err(|_| fail(StatusCode::BAD_GATEWAY, "读取远程安装状态失败。"))?;
    if status != 0 {
        return Err(fail(
            StatusCode::BAD_GATEWAY,
            format!("远程安装脚本失败（{status}）：{}", output.trim()),
        ));
    }
    Ok(output)
}

fn upload(session: &Session, path: &str, bytes: &[u8]) -> ApiResult<()> {
    let sftp = session
        .sftp()
        .map_err(|_| fail(StatusCode::BAD_GATEWAY, "无法打开 SFTP 通道。"))?;
    let mut file = sftp
        .create(Path::new(path))
        .map_err(|error| fail(StatusCode::BAD_GATEWAY, format!("创建远程文件失败：{error}")))?;
    file.write_all(bytes)
        .map_err(|_| fail(StatusCode::BAD_GATEWAY, "写入远程文件失败。"))?;
    file.close()
        .map_err(|_| fail(StatusCode::BAD_GATEWAY, "关闭远程文件失败。"))?;
    Ok(())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> InstallRequest {
        InstallRequest {
            username: "tianyy".to_string(),
            host: "192.168.0.20".to_string(),
            password: "secret".to_string(),
            port: 22,
            node_id: String::new(),
            server_addr: "192.168.10.100:10203".to_string(),
        }
    }

    #[test]
    fn server_address_gets_http_scheme() {
        let value = normalize_server_addr("192.168.10.100:10203").unwrap();
        assert_eq!(value, "http://192.168.10.100:10203");
    }

    #[test]
    fn empty_password_is_rejected() {
        let mut value = request();
        value.password.clear();
        assert!(validate(value).is_err());
    }

    #[test]
    fn whitespace_in_host_is_rejected() {
        let mut value = request();
        value.host = "192.168.0.20 host".to_string();
        assert!(validate(value).is_err());
    }

    #[test]
    fn node_id_with_space_is_rejected() {
        let mut value = request();
        value.node_id = "gpu node".to_string();
        assert!(validate(value).is_err());
    }

    #[test]
    fn installer_never_escalates() {
        let source = include_str!("install.rs");
        let escalate = concat!("su", "do");
        let system_scope = concat!("systemctl --", "system");
        let usr_local = concat!("/usr", "/local/");
        assert!(!source.contains(escalate), "the onboarding path must stay unprivileged");
        assert!(!source.contains(system_scope), "only user units are supported");
        assert!(!source.contains(usr_local), "nothing may be written outside HOME");
        assert!(source.contains("systemctl --user"));
        assert!(source.contains("$HOME"));
    }
}
