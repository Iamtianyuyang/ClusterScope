//! 验收测试:`features/no_root_agent_config.feature`(无 root 部署下 agent 的配置加载与身份文件)。
//!
//! 场景名 ↔ 测试名是硬契约:每个测试函数名逐字等于场景名(闸门把两边都转小写、折叠空白后做
//! 子串匹配),并且都通过**真二进制**驱动,断言的是进程退出码与用户可见的输出。
//!
//! 覆盖约束:`FIX-02` / `FIX-03` / `FIX-04` / `FIX-05`。

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 被测二进制(集成测试由 cargo 提供路径)。
const AGENT_BIN: &str = env!("CARGO_BIN_EXE_clusterscope-agent");

/// 不带 `-c` 时的默认配置路径,与 `config_loader::DEFAULT_CONFIG_PATH` 一致。
const DEFAULT_CONFIG_PATH: &str = "/etc/clusterscope/agent.yaml";

/// 一次性 scratch HOME,测试结束自动清理。
struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "clusterscope-agent-accept-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create scratch home");
        Self { dir }
    }

    fn dir(&self) -> &Path {
        &self.dir
    }

    fn file(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }

    /// 写一个文件(必要时建父目录),返回可放进 YAML / argv 的路径字符串。
    fn write(&self, rel: &str, body: &str) -> String {
        let path = self.file(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent directory");
        }
        fs::write(&path, body).expect("write file");
        slashify(&path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn slashify(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// 一次 agent 进程运行的结果。
struct Outcome {
    /// 退出码;`None` = 等满 grace 时间后进程仍在运行。
    exit: Option<i32>,
    /// stdout + stderr 合并。
    output: String,
    still_running: bool,
}

/// 启动 agent(真进程),等它退出或等满 `grace`,收集输出。
fn run_agent(args: &[&str], home: &Path, grace: Duration) -> Outcome {
    let mut child = Command::new(AGENT_BIN)
        .args(args)
        .env("HOME", home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_STATE_HOME")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn clusterscope-agent");

    let mut stdout = child.stdout.take().expect("stdout pipe");
    let mut stderr = child.stderr.take().expect("stderr pipe");
    let out_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });

    let deadline = Instant::now() + grace;
    let mut exit = None;
    while Instant::now() < deadline {
        match child.try_wait().expect("try_wait") {
            Some(status) => {
                exit = Some(status.code().unwrap_or(-1));
                break;
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }

    let still_running = exit.is_none();
    if still_running {
        let _ = child.kill();
        let _ = child.wait();
    }

    let output = format!(
        "{}{}",
        out_thread.join().unwrap_or_default(),
        err_thread.join().unwrap_or_default()
    );
    Outcome {
        exit,
        output,
        still_running,
    }
}

/// 输出里是否说了"文件不存在"(接受规格里列出的同义措辞)。
fn says_missing(output: &str) -> bool {
    let lower = output.to_lowercase();
    lower.contains("not found")
        || lower.contains("no such file")
        || lower.contains("does not exist")
        || output.contains("不存在")
}

#[test]
fn agent_refuses_to_start_when_the_explicit_config_file_is_missing() {
    let scratch = Scratch::new("explicit-missing");
    // 位于临时目录、连父目录都不存在
    let config = slashify(&scratch.file("absent-dir/agent.yaml"));
    assert!(!Path::new(&config).exists());

    let outcome = run_agent(&["-c", &config], scratch.dir(), Duration::from_secs(15));

    assert!(
        outcome.exit.is_some(),
        "显式 -c 缺失时进程必须以非 0 退出码结束,实际仍在运行:\n{}",
        outcome.output
    );
    assert_ne!(outcome.exit, Some(0), "退出码必须非 0:\n{}", outcome.output);
    assert!(
        outcome.output.contains(&config),
        "输出必须逐字点名不存在的配置路径 {config}:\n{}",
        outcome.output
    );
    assert!(
        says_missing(&outcome.output),
        "输出必须说明该配置文件不存在:\n{}",
        outcome.output
    );
}

#[test]
fn missing_config_error_points_at_the_user_level_config_path() {
    let scratch = Scratch::new("user-level-hint");
    let config = slashify(&scratch.file("absent-dir/agent.yaml"));

    let outcome = run_agent(&["-c", &config], scratch.dir(), Duration::from_secs(15));

    assert!(
        outcome.output.contains(".config/clusterscope/agent.yaml"),
        "错误文案必须提到用户级安装的配置位置 .config/clusterscope/agent.yaml:\n{}",
        outcome.output
    );
}

#[test]
fn missing_config_is_not_silently_replaced_by_the_default_server_address() {
    let scratch = Scratch::new("no-silent-fallback");
    let config = slashify(&scratch.file("absent-dir/agent.yaml"));

    let outcome = run_agent(&["-c", &config], scratch.dir(), Duration::from_secs(15));

    assert!(
        !outcome.output.contains("http://localhost:50051"),
        "显式路径缺失时不得出现默认服务器地址(静默回退):\n{}",
        outcome.output
    );
    assert!(
        !outcome.output.contains("ClusterScope Agent starting"),
        "不得声称自己已经按默认配置启动:\n{}",
        outcome.output
    );
}

#[test]
fn agent_warns_about_the_missing_default_config_file_and_keeps_running() {
    if Path::new(DEFAULT_CONFIG_PATH).exists() {
        // 这台机器上有 /etc/clusterscope/agent.yaml,"默认路径缺失"的前提不成立
        // (qa/harness/no-root-fixes-checks.sh 的 F3 同样跳过;语义由 config_loader 的单测守住)。
        eprintln!("跳过:本机存在 {DEFAULT_CONFIG_PATH}");
        return;
    }
    let scratch = Scratch::new("default-missing");

    let outcome = run_agent(&[], scratch.dir(), Duration::from_secs(5));

    assert!(
        outcome.still_running,
        "不带 -c 时 agent 不得被硬错误终止(守住 NR-06):\n{}",
        outcome.output
    );
    assert!(
        outcome.output.contains("ClusterScope Agent starting"),
        "输出必须有启动横幅:\n{}",
        outcome.output
    );
    assert!(
        outcome.output.contains(DEFAULT_CONFIG_PATH),
        "输出必须点名默认配置路径 {DEFAULT_CONFIG_PATH}:\n{}",
        outcome.output
    );
    assert!(
        says_missing(&outcome.output) && outcome.output.contains("built-in defaults"),
        "输出必须说明没找到并改用内置默认值:\n{}",
        outcome.output
    );
}

#[test]
fn agent_creates_the_missing_parent_directory_of_the_node_identity_file() {
    let scratch = Scratch::new("identity-parent");
    assert!(
        !scratch.file(".config").exists(),
        "前提:干净 HOME,没有 .config 目录"
    );
    // 有效配置,未显式指定 node_id_file -> 用默认 "$HOME/.config/node_id"
    let config = scratch.write(
        "conf/agent.yaml",
        "server_addr: \"http://127.0.0.1:59999\"\nnode_id: \"\"\nreport_interval_secs: 30\n",
    );

    let outcome = run_agent(&["-c", &config], scratch.dir(), Duration::from_secs(5));

    let id_file = scratch.file(".config/node_id");
    assert!(
        id_file.is_file(),
        "$HOME/.config/node_id 未被创建:\n{}",
        outcome.output
    );
    let id = fs::read_to_string(&id_file).expect("read node_id");
    assert!(!id.trim().is_empty(), "node_id 内容必须非空");
    assert!(
        !outcome.output.contains("Failed to write node identity"),
        "输出里不得再出现 Failed to write node identity:\n{}",
        outcome.output
    );
    assert!(
        outcome.still_running,
        "进程在 5 秒后必须仍在运行:\n{}",
        outcome.output
    );
}

#[test]
fn configured_server_address_is_the_address_the_agent_uses() {
    let scratch = Scratch::new("configured-addr");
    let config = scratch.write(
        "agent.yaml",
        "server_addr: \"http://127.0.0.1:59999\"\nnode_id: \"accept-config-addr\"\nreport_interval_secs: 30\n",
    );

    let outcome = run_agent(&["-c", &config], scratch.dir(), Duration::from_secs(3));

    assert!(
        outcome.output.contains("http://127.0.0.1:59999"),
        "启动横幅必须显示配置里的地址:\n{}",
        outcome.output
    );
    assert!(
        !outcome.output.contains("http://localhost:50051"),
        "不得出现默认地址(配置被忽略):\n{}",
        outcome.output
    );
}
