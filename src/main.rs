use nix::{
    errno::Errno,
    libc,
    sys::{
        ptrace,
        wait::{waitpid, WaitPidFlag},
    },
    unistd::Pid,
};
use small_fixed_array::FixedString;
use std::io::{Error as IOError, ErrorKind, Write as _};

struct Process {
    pid: Pid,
    cmdline: FixedString<u8>,
}

impl Process {
    pub fn new(pid: Pid) -> Result<Self, IOError> {
        let cmdline = std::fs::read_to_string(format!("/proc/{pid}/cmdline"))?;
        let cmdline = FixedString::from_string_trunc(cmdline);

        Ok(Self { pid, cmdline })
    }
}

fn enumerate_processes() -> Result<Vec<Process>, IOError> {
    let self_pid = Pid::this();
    let mut pids = Vec::new();

    for proc_file in std::fs::read_dir("/proc")? {
        let os_file_name = proc_file?.file_name();
        let Some(file_name) = os_file_name.to_str() else {
            continue;
        };

        let Ok(pid) = file_name.parse::<libc::pid_t>().map(Pid::from_raw) else {
            continue;
        };

        let process = match Process::new(pid) {
            Err(err) if err.raw_os_error() == Some(Errno::ESRCH as _) => continue,
            val => val,
        }?;

        if pid != self_pid && !process.cmdline.contains("reclaim-memory") {
            pids.push(process);
        }
    }

    Ok(pids)
}

fn get_malloc_trim_addr(pid: Pid) -> Result<u64, IOError> {
    let gdb = std::process::Command::new("gdb")
        .arg("-batch")
        .args(["-ex", &format!("attach {pid}")])
        .args(["-ex", "p malloc_trim"])
        .args(["-ex", "quit"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .spawn()?;

    let output = gdb.wait_with_output()?;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some((addr, _)) = line
            .strip_prefix("$1 = {<text variable, no debug info>} 0x")
            .and_then(|s| s.split_once(' '))
        {
            return Ok(u64::from_str_radix(addr, 16).expect("gdb to print valid hex ptr"));
        }
    }

    Err(IOError::new(
        ErrorKind::Other,
        "gdb did not provide malloc_trim addr",
    ))
}

fn ignore_proc_death<F: FnOnce() -> Result<(), Errno>>(f: F) -> Result<(), Errno> {
    match f() {
        Err(err) if matches!(err, Errno::EPERM | Errno::ESRCH) => {
            eprintln!("Process died before could attach: {err}");
            Ok(())
        }
        val => val,
    }
}

fn trim_process(Process { pid, cmdline }: Process) -> Result<(), IOError> {
    let malloc_trim_addr = get_malloc_trim_addr(pid);
    std::process::exit(0);

    ignore_proc_death(|| {
        println!("Seizing PID {pid}; Name {cmdline}");
        ptrace::seize(pid, ptrace::Options::empty())?;

        println!("Sending interupt");
        ptrace::interrupt(pid)?;

        println!("Waiting for process to stop");
        waitpid(pid, Some(WaitPidFlag::WUNTRACED))?;

        println!("Interrupted, getting regs");
        dbg!(ptrace::getregs(pid)?);

        println!("Got regs, detaching");
        ptrace::detach(pid, None)?;

        Ok(())
    })?;

    Ok(())
}

fn main() -> Result<(), IOError> {
    let pid = std::env::args()
        .nth(1)
        .expect("cmd args should contain a pid")
        .parse()
        .expect("Should be an int");

    let process = Process::new(Pid::from_raw(pid))?;
    trim_process(process)?;

    Ok(())
}
