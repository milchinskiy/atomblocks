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
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "atomblocks-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let fixture = Self(path);
        fs::write(fixture.0.join("config.toml"), CONFIG).unwrap();
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
    // Receiving a record before process termination also verifies pipe flushing.
    assert_eq!(lines.recv_timeout(TIMEOUT).unwrap(), "alpha\n");
    assert!(matches!(
        lines.recv_timeout(Duration::from_millis(400)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert!(process.0.try_wait().unwrap().is_none());
    assert_eq!(wm_name(&conn, root), b"sentinel");

    let mut hit = Process(
        Command::new(env!("CARGO_BIN_EXE_atomblocks"))
            .args(["hit", "1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    assert!(hit.wait().success());
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
fn other_stdout_errors_exit_unsuccessfully_and_report_to_stderr() {
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
        stderr.contains("ERROR"),
        "missing error diagnostic: {stderr}"
    );
    assert!(!stderr.contains("panicked"));
}
