//! Run serially against an isolated X server (see README.md).
use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::xproto::{AtomEnum, ConnectionExt, PropMode},
    wrapper::ConnectionExt as _,
};

const TIMEOUT: Duration = Duration::from_secs(5);
const CONFIG: &str = r#"
delimiter = " | "
[[block]]
execute = "printf 'alpha\\n'"
interval = 0.1
[[block]]
execute = "printf 'β\\r\\n'"
before = "["
after = "]"
"#;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        Self::with_config(CONFIG)
    }

    fn with_config(config: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "atomblocks-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let fixture = Self(path);
        fs::write(fixture.0.join("config.toml"), config).unwrap();
        fixture
    }

    fn spawn(&self, args: &[&str], stdout: Stdio) -> Process {
        let child = Command::new(env!("CARGO_BIN_EXE_atomblocks"))
            .args(args)
            .arg("--config")
            .arg(self.0.join("config.toml"))
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(File::create(self.0.join("stderr")).unwrap())
            .spawn()
            .unwrap();
        Process(child)
    }

    fn stderr(&self) -> String {
        fs::read_to_string(self.0.join("stderr")).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Process(Child);

impl Process {
    fn lines(&mut self) -> mpsc::Receiver<String> {
        let stdout = self.0.stdout.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if sender.send(line).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        receiver
    }

    fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "process did not exit");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wm_name(conn: &impl Connection, root: u32) -> Vec<u8> {
    conn.get_property(false, root, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)
        .unwrap()
        .reply()
        .unwrap()
        .value
}

fn hit(id: u32) {
    let status = Command::new(env!("CARGO_BIN_EXE_atomblocks"))
        .arg("hit")
        .arg(id.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
#[ignore = "requires isolated X11; run serially with xvfb-run"]
fn stdout_is_clean_preserves_wm_name_and_supports_hits() {
    let (conn, screen) = x11rb::connect(None).unwrap();
    let root = conn.setup().roots[screen].root;
    conn.change_property8(
        PropMode::REPLACE,
        root,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        b"sentinel",
    )
    .unwrap()
    .check()
    .unwrap();

    let fixture = Fixture::new();
    let mut process = fixture.spawn(&["--trace", "run", "--stdout"], Stdio::piped());
    let lines = process.lines();
    assert_eq!(lines.recv_timeout(TIMEOUT).unwrap(), "alpha\n");
    assert!(matches!(
        lines.recv_timeout(Duration::from_millis(400)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert!(process.0.try_wait().unwrap().is_none());
    assert_eq!(wm_name(&conn, root), b"sentinel");

    hit(1);
    assert_eq!(lines.recv_timeout(TIMEOUT).unwrap(), "alpha | [β]\n");
    assert_eq!(wm_name(&conn, root), b"sentinel");
    assert!(fixture.stderr().contains("Ready to receive events"));
    assert!(!fixture.stderr().contains("Updating WM_NAME"));
}

#[test]
#[ignore = "requires isolated X11; run serially with xvfb-run"]
fn default_output_preserves_raw_wm_name_and_keeps_stdout_empty() {
    let (conn, screen) = x11rb::connect(None).unwrap();
    let root = conn.setup().roots[screen].root;
    conn.delete_property(root, AtomEnum::WM_NAME.into())
        .unwrap()
        .check()
        .unwrap();
    let fixture = Fixture::new();
    let mut process = fixture.spawn(&["--verbose", "run"], Stdio::piped());
    let lines = process.lines();
    let deadline = Instant::now() + TIMEOUT;
    while wm_name(&conn, root) != b"alpha\n" {
        assert!(Instant::now() < deadline, "WM_NAME was not updated");
        assert!(process.0.try_wait().unwrap().is_none());
        thread::sleep(Duration::from_millis(10));
    }
    assert!(matches!(
        lines.recv_timeout(Duration::from_millis(200)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert!(fixture.stderr().contains("Updating WM_NAME"));
}

#[test]
#[ignore = "requires isolated X11; run serially with xvfb-run"]
fn slow_block_does_not_delay_fast_block() {
    let fixture = Fixture::with_config(
        r#"
delimiter = " | "
[[block]]
execute = "sleep 1; printf slow"
interval = 60
[[block]]
execute = "printf fast"
interval = 60
"#,
    );
    let mut process = fixture.spawn(&["run", "--stdout"], Stdio::piped());
    let lines = process.lines();
    assert_eq!(
        lines.recv_timeout(Duration::from_millis(700)).unwrap(),
        "fast\n"
    );
    assert_eq!(lines.recv_timeout(TIMEOUT).unwrap(), "slow | fast\n");
}

#[test]
#[ignore = "requires isolated X11; run serially with xvfb-run"]
fn zero_interval_is_manual_only() {
    let fixture = Fixture::with_config(
        r#"
[[block]]
execute = "printf manual"
interval = 0
"#,
    );
    let mut process = fixture.spawn(&["run", "--stdout"], Stdio::piped());
    let lines = process.lines();
    assert!(matches!(
        lines.recv_timeout(Duration::from_millis(350)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    hit(0);
    assert_eq!(lines.recv_timeout(TIMEOUT).unwrap(), "manual\n");
}

#[test]
#[ignore = "requires isolated X11; run serially with xvfb-run"]
fn startup_drains_hit_queues_larger_than_one_chunk() {
    let (conn, screen) = x11rb::connect(None).unwrap();
    let root = conn.setup().roots[screen].root;
    let atoms = atomblocks::atoms::AtomBlocksAtoms::new(&conn)
        .unwrap()
        .reply()
        .unwrap();
    let mut ids = vec![0_u32; 1024];
    ids.push(1);
    conn.change_property32(
        PropMode::REPLACE,
        root,
        atoms._ATOMBLOCKS_HIT_QUEUE,
        AtomEnum::INTEGER,
        &ids,
    )
    .unwrap()
    .check()
    .unwrap();

    let fixture = Fixture::with_config(
        r#"
delimiter = " | "
[[block]]
execute = "printf zero"
interval = 0
[[block]]
execute = "printf one"
interval = 0
"#,
    );
    let mut process = fixture.spawn(&["run", "--stdout"], Stdio::piped());
    let lines = process.lines();
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "large startup hit queue was not drained"
        );
        let line = lines.recv_timeout(remaining).unwrap();
        if line == "zero | one\n" {
            break;
        }
    }
}

#[test]
#[ignore = "requires isolated X11; run serially with xvfb-run"]
fn child_stdin_is_null() {
    let fixture = Fixture::with_config(
        r#"
[[block]]
execute = "cat; printf done"
interval = 60
"#,
    );
    let mut process = fixture.spawn(&["run", "--stdout"], Stdio::piped());
    let lines = process.lines();
    assert_eq!(lines.recv_timeout(TIMEOUT).unwrap(), "done\n");
}

#[test]
#[ignore = "requires isolated X11; run serially with xvfb-run"]
fn nonzero_exit_keeps_stdout_and_reports_stderr() {
    let fixture = Fixture::with_config(
        r#"
[[block]]
execute = "printf broken; printf diagnostic >&2; exit 7"
interval = 60
"#,
    );
    let mut process = fixture.spawn(&["run", "--stdout"], Stdio::piped());
    let lines = process.lines();
    assert_eq!(lines.recv_timeout(TIMEOUT).unwrap(), "broken\n");

    let deadline = Instant::now() + TIMEOUT;
    loop {
        let stderr = fixture.stderr();
        if stderr.contains("diagnostic") && stderr.contains("status: 7") {
            break;
        }
        assert!(Instant::now() < deadline, "missing diagnostic: {stderr}");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "requires isolated X11; run serially with xvfb-run"]
fn output_limit_preserves_previous_cell_and_reports_failure() {
    let fixture = Fixture::with_config(
        r#"
[[block]]
execute = "printf 12345"
interval = 60
output_limit = 4
"#,
    );
    let mut process = fixture.spawn(&["run", "--stdout"], Stdio::piped());
    let lines = process.lines();
    assert!(matches!(
        lines.recv_timeout(Duration::from_millis(350)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    let deadline = Instant::now() + TIMEOUT;
    while !fixture.stderr().contains("output_limit") {
        assert!(Instant::now() < deadline, "missing output-limit diagnostic");
        thread::sleep(Duration::from_millis(10));
    }
    assert!(process.0.try_wait().unwrap().is_none());
}

#[test]
#[ignore = "requires isolated X11; run serially with xvfb-run"]
fn invalid_interval_is_reported_without_panicking() {
    let fixture = Fixture::with_config(
        r#"
[[block]]
execute = "printf nope"
interval = -1
"#,
    );
    let mut process = fixture.spawn(&["run", "--stdout"], Stdio::null());
    assert!(!process.wait().success());
    let stderr = fixture.stderr();
    assert!(stderr.contains("block 0"), "{stderr}");
    assert!(stderr.contains("interval"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

#[test]
#[ignore = "requires isolated X11; run serially with xvfb-run"]
fn closed_stdout_pipe_exits_successfully_without_diagnostics() {
    let fixture = Fixture::new();
    let mut process = fixture.spawn(&["run", "--stdout"], Stdio::piped());
    drop(process.0.stdout.take());
    assert!(process.wait().success());
    assert_eq!(fixture.stderr(), "");
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires isolated X11; run serially with xvfb-run"]
fn other_stdout_errors_exit_unsuccessfully_and_report_the_output_error() {
    let fixture = Fixture::new();
    let mut process = fixture.spawn(
        &["run", "--stdout"],
        File::options()
            .write(true)
            .open("/dev/full")
            .unwrap()
            .into(),
    );
    assert!(!process.wait().success());
    let stderr = fixture.stderr();
    assert!(
        stderr.contains("I/O error"),
        "missing output diagnostic: {stderr}"
    );
    assert!(!stderr.contains("panicked"));
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires isolated X11; run serially with xvfb-run"]
fn sigterm_terminates_and_reaps_the_block_process_group() {
    let fixture = Fixture::with_config(
        r#"
[[block]]
execute = "echo $$ > child.pid; sleep 30"
interval = 60
"#,
    );
    let mut command = Command::new(env!("CARGO_BIN_EXE_atomblocks"));
    command
        .current_dir(&fixture.0)
        .args(["run", "--stdout", "--config"])
        .arg(fixture.0.join("config.toml"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(File::create(fixture.0.join("stderr")).unwrap());
    let mut process = Process(command.spawn().unwrap());

    let pid_file = fixture.0.join("child.pid");
    let deadline = Instant::now() + TIMEOUT;
    while !pid_file.exists() {
        assert!(Instant::now() < deadline, "child pid was not recorded");
        thread::sleep(Duration::from_millis(10));
    }
    let child_pid: i32 = fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();

    assert_eq!(
        unsafe { libc::kill(process.0.id() as i32, libc::SIGTERM) },
        0
    );
    let status = process.wait();
    assert_eq!(status.code(), Some(128 + libc::SIGTERM));

    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let result = unsafe { libc::kill(child_pid, 0) };
        if result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "child process group survived SIGTERM"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
