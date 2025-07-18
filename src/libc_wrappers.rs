// This file is just a libc wrapper to keep the main code cleaner.
// While it doesn't contain any Docker-specific code, here are a few things to keep in mind:
// - File descriptors close automatically when variables go out of scope.
//
// I wrote this file only because the existing nix library doesn’t support the new Linux API.

use std::{ffi::CString, io::Error, os::raw::c_void, path::PathBuf};

use libc::c_char;

use crate::common::WithErrExt;

pub struct FileDescriptor {
    fd: i32
}
pub type Pid = i32;

impl Drop for FileDescriptor {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}

// https://github.com/brauner/man-pages-md/blob/main/fsopen.md
pub fn fsopen(fs_name: &str, flags: u32) -> Result<FileDescriptor, String> {
    let fs_name = create_c_string(&fs_name).with_err("invald fs_name")?;
    unsafe {
        let fd = libc::syscall(libc::SYS_fsopen, fs_name.as_ptr(), flags);
        if fd == -1 {
            return Err(format!("faild to open fs configuration context: {}", Error::last_os_error()));
        }
        Ok(FileDescriptor { fd: fd as i32 })
    }
}

// https://github.com/torvalds/linux/blob/155a3c003e555a7300d156a5252c004c392ec6b0/include/uapi/linux/mount.h#L98
pub enum FsconfigCommand {
    SetString = 1,
    CmdCreate = 6,
}

// https://github.com/brauner/man-pages-md/blob/main/fsconfig.md
pub fn fsconfig(fd: &FileDescriptor, cmd: FsconfigCommand, key: Option<&str>, value: Option<&str>, aux: i32) -> Result<(), String> {
    let cmd: u32 = cmd as u32;
    let key = if let Some(key) = key { Some(create_c_string(key).with_err("invalid key")?) } else { Option::None };
    let value = if let Some(value) = value { Some(create_c_string(value).with_err("invalid value")?) } else { Option::None };
    let key_ptr = if let Some(key) = &key { key.as_ptr() } else { std::ptr::null() };
    let value_ptr = if let Some(value) = &value { value.as_ptr() } else { std::ptr::null() };
    unsafe {
        let result = libc::syscall(libc::SYS_fsconfig, fd.fd, cmd, key_ptr, value_ptr, aux);
        if result == -1 {
            return Err(format!("failed to configure fs context: {}", Error::last_os_error()));
        }
    }
    Ok(())
}

// https://github.com/brauner/man-pages-md/blob/main/fsmount.md
pub fn fsmount(fd: &FileDescriptor, flags: u32, mount_attrs: u32) -> Result<FileDescriptor, String> {
    unsafe {
        let mnt_fd = libc::syscall(libc::SYS_fsmount, fd.fd, flags, mount_attrs);
        if mnt_fd == -1 {
            return Err(format!("failed to mount: {}", Error::last_os_error()));
        }

        Ok(FileDescriptor { fd: mnt_fd as i32 })
    }
}

// https://github.com/brauner/man-pages-md/blob/main/move_mount.md
pub fn move_mount(
    from_dirfd: Option<&FileDescriptor>,
    from_pathname: Option<&str>,
    to_dirfd: Option<&FileDescriptor>,
    to_pathname: Option<&str>,
    flags: u32
) -> Result<(), String> {
    let from_dirfd = if let Some(v) = from_dirfd { v.fd } else { libc::AT_FDCWD };
    let to_dirfd = if let Some(v) = to_dirfd { v.fd } else { libc::AT_FDCWD };
    let from_pathname = if let Some(v) = from_pathname { v } else { "" };
    let to_pathname = if let Some(v) = to_pathname { v } else { "" };

    let from_pathname = create_c_string(from_pathname).with_err("invalid from_pathname")?;
    let to_pathname = create_c_string(to_pathname).with_err("invalid to_pathname")?;

    unsafe {
        if libc::syscall(libc::SYS_move_mount, from_dirfd, from_pathname.as_ptr(), to_dirfd, to_pathname.as_ptr(), flags) == -1 {
            return Err(format!("failed to move mount: {}", Error::last_os_error()));
        }
    }

    return Ok(())
}

// https://man7.org/linux/man-pages/man2/pivot_root.2.html
pub fn pivot_root(new_root: &PathBuf, put_old: &PathBuf) -> Result<(), String> {
    let new_root = create_c_string(&new_root.to_string_lossy()).with_err("invalid new_root")?;
    let put_old = create_c_string(&put_old.to_string_lossy()).with_err("invalid put_old")?;

    unsafe {
        if libc::syscall(libc::SYS_pivot_root, new_root.as_ptr(), put_old.as_ptr()) == -1 {
            return Err(format!("failed to pivot root: {}", Error::last_os_error()));
        }
    }
    return Ok(());
}

pub fn chroot(target: &str) -> Result<(), String> {
    let target = create_c_string(target).with_err("invalid target")?;
    unsafe {
        if libc::chdir(target.as_ptr()) == -1 {
            return Err(format!("chroot failed: {}", Error::last_os_error()));
        }
    }
    return Ok(());
}

// https://man7.org/linux/man-pages/man2/mknod.2.html
pub fn mknod(pathname: &str, mode: libc::mode_t, dev: libc::dev_t) -> Result<(), String> {
    let pathname = create_c_string(pathname).with_err("invalid pathname")?;
    unsafe {
        if libc::mknod(pathname.as_ptr(), mode, dev) == -1 {
            return Err(format!("mknod failed: {}", Error::last_os_error()));
        }
    }
    return Ok(());
}

// https://man7.org/linux/man-pages/man2/mount.2.html
pub fn mount(source: &str, target: &str, fstype: &str, flags: u64, data: Option<&str>) -> Result<(), String> {
    let source = create_c_string(source).with_err("invalid source")?;
    let target = create_c_string(target).with_err("invalid target")?;
    let fstype = create_c_string(fstype).with_err("invalid fstype")?;
    let data = if let Some(v) = data { Some(create_c_string(v).with_err("invalid data")?) } else { Option::None };
    unsafe {
        if libc::mount(
            source.as_ptr(),
            target.as_ptr(),
            fstype.as_ptr(),
            flags,
            if let Some(v) = data { v.as_ptr() as *const c_void } else { std::ptr::null() }
        ) == -1 {
            return Err(format!("mount failed: {}", Error::last_os_error()));
        }
    }
    Ok(())
}

// https://man7.org/linux/man-pages/man2/umount.2.html
pub fn umount(target: &str, flags: i32) -> Result<(), String> {
    let target = create_c_string(target).with_err("invalid target")?;
    unsafe {
        if libc::umount2(target.as_ptr(), flags) == -1 {
            return Err(format!("umount failed: {}", Error::last_os_error()));
        }
    }
    return Ok(());
}

// https://man7.org/linux/man-pages/man2/execve.2.html#RETURN_VALUE
pub fn execve(cmd: String, args: Vec<String>, envs: Vec<String>) -> Result<(), String> {
    let cmd: CString = create_c_string(&cmd).with_err("cmd invalud")?;
    let (args, args_pointers) = create_c_string_array(args).with_err("args invalid")?;
    let (envs, envs_pointers) = create_c_string_array(envs).with_err("environment vars invalid")?;

    unsafe {
        if libc::execve(cmd.as_ptr(), args_pointers.as_ptr(), envs_pointers.as_ptr()) == -1 {
            return Err(format!("execve failed: {}", std::io::Error::last_os_error()).into());
        }
    }

    // Ensure Rust doesn’t drop the execve argument arrays before the execve called.
    std::mem::drop(args);
    std::mem::drop(envs);

    return Ok(());
}

// https://man7.org/linux/man-pages/man2/getpid.2.html
pub fn getpid() -> Pid {
    return unsafe { libc::getpid() };
}

// https://man7.org/linux/man-pages/man3/wait.3p.html
// this function is oversimplified
pub fn waitpid(pid: Pid, options: i32) -> Result<(i32, i32), Error> {
    unsafe {
        let mut stat_loc: i32 = 0;
        let result = libc::waitpid(pid,  &mut stat_loc, options);
        match result {
            0 => return Err(Error::new(std::io::ErrorKind::Unsupported, format!("unknown state: {result}"))),
            -1 => return Err(Error::last_os_error()),
            pid => return Ok((pid, stat_loc)),
        }
    }
}

// https://man7.org/linux/man-pages/man2/fork.2.html
pub fn fork() -> Result<Pid, String> {
    unsafe {
        let result = libc::fork();
        if result == -1 {
            return Err(format!("fork failed: {}", Error::last_os_error()));
        }
        Ok(result)
    }
}

// https://man7.org/linux/man-pages/man2/unshare.2.html
pub fn unshare(flags: i32) -> Result<(), String> {
    unsafe {
        if libc::unshare(flags) == -1 {
            return Err(format!("unshare failed: {}", Error::last_os_error()));
        }
    }
    return Ok(());
}

// https://man7.org/linux/man-pages/man2/eventfd.2.html
pub fn eventfd(init: u32, flags: i32) -> Result<FileDescriptor, String> {
    unsafe {
        let result = libc::eventfd(init, flags);
        if result == -1 {
            return Err(format!("eventfd failed: {}", Error::last_os_error()));
        }
        return Ok(FileDescriptor { fd: result });
    }
}

pub fn eventfd_read(fd: &FileDescriptor) -> Result<u64, String> {
    unsafe {
        let mut value: u64 = 0;
        let result = libc::eventfd_read(fd.fd, &mut value);
        if result == -1 {
            return Err(format!("eventfd_read failed: {}", Error::last_os_error()));
        }
        return Ok(value);
    }
}

pub fn eventfd_write(fd: &FileDescriptor, value: u64) -> Result<(), String> {
    unsafe {
        let result = libc::eventfd_write(fd.fd, value);
        if result == -1 {
            return Err(format!("eventfd_read failed: {}", Error::last_os_error()));
        }
    }
    return Ok(());
}

// https://man7.org/linux/man-pages/man2/epoll_create.2.html
pub fn epoll_create1(flags: i32) -> Result<FileDescriptor, String> {
    unsafe {
        let result = libc::epoll_create1(flags);
        if result == -1 {
            return Err(format!("epoll_create1 failed: {}", Error::last_os_error()));
        }
        return Ok(FileDescriptor { fd: result });
    }
}

pub fn prctl(options: i32) -> Result<(), String> {
    unsafe {
        let result = libc::prctl(options);
        if result == -1 {
            return Err(format!("prctl failed: {}", Error::last_os_error()));
        }
    }
    return Ok(());
}

// https://man7.org/linux/man-pages/man3/posix_openpt.3.html
pub fn posix_openpt() -> Result<FileDescriptor, String> {
    unsafe {
        let result = libc::posix_openpt(libc::O_RDWR);
        if result == -1 {
            return Err(format!("posix_openpt failed: {}", Error::last_os_error()));
        }
        return Ok(FileDescriptor { fd: result });
    }
}

// https://man7.org/linux/man-pages/man3/grantpt.3.html
pub fn grantpt(fd: &FileDescriptor) -> Result<(), String> {
    unsafe {
        if libc::grantpt(fd.fd) == -1 {
            return Err(format!("grantpt failed: {}", Error::last_os_error()));
        }
    }
    return Ok(());
}

// https://man7.org/linux/man-pages/man3/unlockpt.3.html
pub fn unlockpt(fd: &FileDescriptor) -> Result<(), String> {
    unsafe {
        if libc::unlockpt(fd.fd) == -1 {
            return Err(format!("unlockpt failed: {}", Error::last_os_error()));
        }
    }
    return Ok(());
}

// pub fn ptsname() -> Result<(), String> {
//     unsafe {
//         if libc::ptsname(fd.fd) == -1 {
//             return Err(format!("ptsname failed: {}", Error::last_os_error()));
//         }
//     }
//     return Ok(());
// }

// https://man7.org/linux/man-pages/man2/geteuid.2.html
pub fn getuid() -> libc::uid_t {
    unsafe {
        return libc::getuid();
    }
}

// custom function
pub fn get_username(uid: libc::uid_t) -> Result<Option<String>, String> {
    unsafe {
        let result = libc::getpwuid(uid);
        if result.is_null() {
            return Err("getpwuid return null".to_owned());
        }
        let t = CString::from_raw((*result).pw_name);
        let name = t.to_string_lossy();
        return Ok(Some(name.into_owned()));
    }
}

// utilities
fn create_c_string(value: &str) -> Result<CString, String> {
    match CString::new(value) {
        Ok(v) => Ok(v),
        Err(e) => Err(format!("failed to create CString from: '{}', error: {}", value, e))
    }
}

fn create_c_string_array(values: Vec<String>) -> Result<(Vec<CString>, Vec<*const c_char>), String> {
    let array: Vec<CString> = values.into_iter()
        .map(|x| CString::new(x).unwrap())
        .collect();

    let mut pointers: Vec<*const c_char> = array.iter()
        .map(|x| x.as_ptr())
        .collect();
    pointers.push(std::ptr::null());

    Ok((array, pointers))
}
