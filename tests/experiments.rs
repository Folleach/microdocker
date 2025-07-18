use std::{error::Error, ffi::{c_void, CString}, fs::{self, OpenOptions}, io::{self, Read, Write}, os::fd::{FromRawFd, RawFd}, process::Command, thread};

use libc::{execlp, fork, getpid, getpwuid, getuid, sleep};

#[test]
pub fn with_pipe() -> Result<(), Box<dyn Error>> {
    unsafe {

        print_username();
        if libc::unshare(libc::CLONE_NEWUSER) == -1 {
            println!("error: {}", io::Error::last_os_error());
            return Ok(());
        }

        let mut pipefd = [0i32; 2];
        if libc::pipe2(pipefd.as_mut_ptr(), 0) == -1 {
            println!("failed to create pipe");
        }
        let child_pid = libc::fork();
        if child_pid == 0 {
            // child execution path
            let mut buf = [0u8; 1];
            libc::read(pipefd[0], buf.as_mut_ptr() as *mut c_void, 1);
            println!("child received a signal");
            print_username();
            return Ok(());
        }

        // parent execution path
        {
            let mut uid_map_file = OpenOptions::new().write(true).open(format!("/proc/{}/uid_map", getpid())).unwrap();
            write!(uid_map_file, "0 1000 1").unwrap();
        }

        println!("send signal to the child");
        let mut buf = [12u8; 1];
        libc::write(pipefd[1], buf.as_mut_ptr() as *const c_void, 1);
    }

    return Ok(());
}

#[test]
pub fn straight() -> Result<(), Box<dyn Error>> {
    unsafe {

        print_username();
        if libc::unshare(libc::CLONE_NEWUSER) == -1 {
            println!("error: {}", io::Error::last_os_error());
            return Ok(());
        }

        // parent execution path
        {
            let mut uid_map_file = OpenOptions::new().write(true).open(format!("/proc/{}/uid_map", getpid())).unwrap();
            write!(uid_map_file, "0 1000 1").unwrap();
        }

        print_username();
    }

    return Ok(());
}

#[test]
fn forking() {
    println!("i am start!");
    let pid = unsafe { fork() };
    if pid == 0 {
        unsafe {
            let app = CString::new("/bin/echo").unwrap();
            let args= [
                CString::new("/bin/echo").unwrap(),
                CString::new("hello").unwrap(),
            ];
            let args_ptrs: Vec<_> = args.iter().map(|s| *s.as_ptr()).collect();
            let s = execlp(app.as_ptr(), args_ptrs.as_ptr());
            println!("executed: {}", s);
            println!("Error: {:?}", std::io::Error::last_os_error());
        }
        println!("i am new, my pid is: {}", unsafe { getpid() });
    }
    else {
        println!("i am old ({}), i spawn: {}", unsafe { getpid() }, pid);
    }

}

#[test]
pub fn pid_namespace_test() {
    unsafe {
        println!("current pid: {}", getpid());

        libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS | libc::CLONE_NEWPID);
        println!("after unshare: {}", getpid());

        let child_pid = fork();
        if child_pid == 0 {
            println!("in a new process: {}", getpid());
            return;
        }

        println!("returned after fork: {}", child_pid);
    }
}

#[test]
fn redirect_stdout() {
    unsafe {
        let mut pipe_fds: [RawFd; 2] = [0; 2];
        if libc::pipe(pipe_fds.as_mut_ptr()) != 0 {
            panic!("pipe failed");
        }

        let read_fd = pipe_fds[0];
        let write_fd = pipe_fds[1];

        if libc::dup2(libc::STDOUT_FILENO, 77) < 0 {
            panic!("dup2 failed");
        }
        if libc::dup2(write_fd, libc::STDOUT_FILENO) < 0 {
            panic!("dup2 failed");
        }

        thread::spawn(move || {
            let mut reader = std::fs::File::from_raw_fd(read_fd);
            let mut writer = std::fs::File::from_raw_fd(77);
            let mut buffer = [0u8; 1024];

            loop {
                let n = match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
                };

                let output = String::from_utf8_lossy(&buffer[..n]);
                for line in output.lines() {
                    writeln!(writer, "[Oh, hello there] {}", line).unwrap();
                }
            }
        });

        println!("Hello, world!");
        println!("Yet another line!");

        thread::sleep(std::time::Duration::from_secs(3));
    }
}

unsafe fn print_username() {
    unsafe {
        let uid = getuid();
        let name = getpwuid(uid);
        println!("you are {}", CString::from_raw((*name).pw_name).to_string_lossy());
    }
}
