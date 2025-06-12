use nix::sys::stat::Mode;
use nix::unistd::ForkResult;
use std::ffi::CString;
use std::path::Path;

use nix::{
    fcntl::OFlag,
    unistd::{Pid, dup2_stderr, dup2_stdin, dup2_stdout},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pipe_path_str = "/tmp/rust_shell.pipe";
    let log_path_str = "/tmp/rust_shell.log";
    let pipe_path = Path::new(pipe_path_str);
    let log_path = Path::new(log_path_str);

    if pipe_path.exists() {
        std::fs::remove_file(pipe_path)?;
    }

    let mode = Mode::S_IRWXU;
    nix::unistd::mkfifo(pipe_path, mode)?;

    match unsafe { nix::unistd::fork() } {
        Ok(ForkResult::Parent { child }) => {
            parent_process(child, pipe_path, log_path);
            Ok(())
        }
        Ok(ForkResult::Child) => child_process(pipe_path, log_path),
        Err(e) => {
            let _ = std::fs::remove_file(pipe_path);
            std::process::exit(e as i32);
        }
    }
}

pub fn child_process(in_file: &Path, out_file: &Path) -> Result<(), Box<dyn std::error::Error>> {
    nix::unistd::setsid()?;

    let in_fd = nix::fcntl::open(in_file, OFlag::O_RDONLY, Mode::empty())?;

    dup2_stdin(&in_fd)?;
    nix::unistd::close(in_fd)?;

    let out_flags = OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_TRUNC;
    let out_mode = Mode::S_IRUSR | Mode::S_IWUSR; // Read/Write for user
    let out_fd = nix::fcntl::open(out_file, out_flags, out_mode)?;

    dup2_stdout(&out_fd)?;
    dup2_stderr(&out_fd)?;
    nix::unistd::close(out_fd)?;

    let shell_path = CString::new("/bin/sh").unwrap();
    let shell_args = [shell_path.clone()];
    let Err(e) = nix::unistd::execvp(&shell_path, &shell_args);
    std::process::exit(e as i32);
}

pub fn parent_process(child_pid: Pid, in_file: &Path, out_file: &Path) {
    println!("input file: {}", in_file.to_string_lossy());
    println!("output file: {}", out_file.to_string_lossy());
    println!("to \"kill {}\"", child_pid);

    std::process::exit(0);
}
