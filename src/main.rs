use bytes::Bytes;
use flate2::{bufread::GzDecoder};
use log::{info, trace};
use microdocker::{
    command_args::{Commands, MicordockerCli}, common::{WithErrExt, create_if_not_exists, get_microdocker_data_directory, init_log}, image_repository::ImageRepository, libc_wrappers::{
        FsconfigCommand, Pid, PollResult, chroot, current_winsize, dup2, eventfd, eventfd_read, eventfd_write, execve, fork, fsconfig, fsmount, fsopen, get_username, getpid, getuid, grantpt, mount, move_mount, open_tree, pidfd_open, pivot_root, poll, prctl, recv_fd, send_fd, set_winsize, setsid, socketpair, symlink, take_slave_from_fd, umount, unlockpt, unshare
    }, models::{Descriptor, Image, ImageConfig, Index, Manifest}, reference::SimpleReference, seccomp::restrict_syscalls, termios_raii::TermiosState
};
use reqwest::{header::HeaderValue, Client, Response};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tar::Archive;
use core::panic;
use std::{
    collections::HashMap, env, error::Error, ffi::CString, fs::{self, File, OpenOptions}, io::{self, BufReader, Read, Seek, SeekFrom, Write}, mem::{self, ManuallyDrop}, os::fd::{AsRawFd, FromRawFd, OwnedFd}, path::PathBuf, process::{Command, Output}, vec
};
use libc::{MOVE_MOUNT_F_EMPTY_PATH, S_IRUSR, S_IWUSR, S_IXUSR, chmod, getgid, pollfd};
use clap::Parser;

const DEFAULT_REGISTRY_ADDRESS: &str = "https://registry-1.docker.io";
const WWW_AUTHENTICATE_HEADER_NAME: &str = "WWW-Authenticate";
const INDEX_SCHEMA: &str = "application/vnd.oci.image.index.v1+json";
const MANIFEST_V2_SCHEMA: &str = "application/vnd.docker.distribution.manifest.v2+json";
const MANIFEST_LIST_V2_SCHEMA: &str = "application/vnd.docker.distribution.manifest.list.v2+json";

/*
data_directory structure:
    ~/.local/share/microdocker/container/<pid>/ - container runtime
    ~/.local/share/microdocker/image/ - image database
    ~/.local/share/microdocker/overlay2/ - image layers
*/

fn main() -> Result<(), Box<dyn std::error::Error>> {

    let cli = MicordockerCli::parse();
    let args = match cli.command {
        Commands::Run(args) => args,
    };

    init_log(args.verbose);

    trace!("create default directories if not exists");
    let data_directory = get_microdocker_data_directory()?;
    trace!("data directory: {}", &data_directory.to_string_lossy());
    let image_directory = create_if_not_exists(data_directory.join("image"), 0o700)?;
    let overlay_directory = create_if_not_exists(data_directory.join("overlay2"), 0o700)?;
    let containers_direcotry2 = create_if_not_exists(data_directory.join("containers"), 0o700)?;

    let pid = getpid();
    let current_container_directory = create_if_not_exists(containers_direcotry2.join(pid.to_string()), 0o700)?;
    let work_dir = create_if_not_exists(current_container_directory.join("work"), 0o700)?;
    let diff_dir = create_if_not_exists(current_container_directory.join("diff"), 0o700)?;
    let merged_dir = create_if_not_exists(current_container_directory.join("merged"), 0o700)?;

    let reference = SimpleReference::parse(&args.reference);

    let images = ImageRepository::new(image_directory);
    let mut image: Option<Image> = images.get(&reference).with_err("failed to get image from local storage")?;

    if let None = image {
        info!("reference {} not found in local storage", &reference.to_string());

        let rt = tokio::runtime::Runtime::new().with_err("failed to create async runtime")?;
        image = rt.block_on(async {
            let manifest = get_manifest(&reference).await?;
            let digest = &manifest.config.as_ref().unwrap().digest;
            let image_reference = reference.with_hash(digest.as_ref().expect("digest is not present"));
            let image_blob = registry_get(&image_reference, "blobs").await?.json::<Image>().await?;
            let packed = download_layers(&reference, &manifest.layers).await;
            if let Err(err) = packed {
                return Err(format!("failed to download layers: {}", err).into());
            }
            unpack_layers(packed.unwrap(), &overlay_directory).await;

            images.append(vec![&reference, &image_reference], &manifest, &image_blob)?;

            trace!("manifest: {:#?}", &manifest);
            trace!("image: {:#?}", image_blob);
            images.get(&reference)
        })?;
    }

    let image = image.ok_or("image not found")?;
    let config = image.config.ok_or("image.config is omitted")?;
    if image.root_fs.fstype != "layers" {
        return Err("unsupported root_fs type, check the https://github.com/opencontainers/image-spec/blob/main/schema/config-schema.json for actual types. only 'layers' supported now".into());
    }

    trace!("prepare container runtime");

    let dev_tty = open_tree(&libc::AT_FDCWD, "/dev/tty", libc::OPEN_TREE_CLONE).with_err("failed to steal /dev/tty from host")?;
    let dev_null = open_tree(&libc::AT_FDCWD, "/dev/null", libc::OPEN_TREE_CLONE).with_err("failed to steal /dev/null from host")?;

    let (upper, lower) = socketpair().with_err("socketpair for terminal fd")?;
    let controller = ControllerProcessContext {
        eventfd_setup_main: eventfd(0, 0).with_err("failed to create main eventfd")?,
        eventfd_setup_intermediate: eventfd(0, 0).with_err("failed to create intermediate eventfd")?,
        socket_terminal_fd_main_side: upper,
        socket_terminal_fd_container_side: lower,
        main_pid: pid,
        has_terminal: true,
        mount_points: DetachedMountPoints {
            dev_tty,
            dev_null
        }
    };

    match fork().with_err("failed to fork process for intermediate setup")? {
        0 => { /* just continue the current function */ },
        child_pid => {
            microdocker_main_process(current_container_directory, child_pid, controller)?;
            return Ok(());
        }
    }

    trace!("unshare the process");
    unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS | libc::CLONE_NEWPID)?;
    eventfd_write(&controller.eventfd_setup_main, 1)?; // request main-process to continue
    eventfd_read(&controller.eventfd_setup_intermediate)?; // wait the main-process to set up uid/gid map to us

    {
        // This block mounting the rootfs, using mount API because mount syscall has a data length limitation,
        // which doesn't work for a large number of layers.
        // https://docs.kernel.org/filesystems/mount_api.html
        //
        // as a workaround we can use symlinks
        // https://github.com/moby/moby/commit/23e5c94cfb26eb72c097892712d3dbaa93ee9bc0

        trace!("setting up the rootfs");
        let lowers: Result<Vec<String>, Box<dyn Error>> = image.root_fs.diff_ids
            .iter()
            .map(|x| {
                let digest = SpecDigest::parse(x)?;
                return Ok(env::current_dir()?.join(&overlay_directory).join(digest.hash).into_os_string().into_string().unwrap());
            })
            .collect();

        let fs_fd = fsopen("overlay", 0)?;
        for lower in lowers?.into_iter().rev() {
            fsconfig(&fs_fd, FsconfigCommand::SetString, Some("lowerdir+"), Some(&lower), 0).with_err("failed to set lower dir")?;
        }
        fsconfig(&fs_fd, FsconfigCommand::SetString, Some("upperdir"), Some(&diff_dir.into_os_string().into_string().unwrap()), 0).with_err("failed to set upper dir")?;
        fsconfig(&fs_fd, FsconfigCommand::SetString, Some("workdir"), Some(&work_dir.into_os_string().into_string().unwrap()), 0).with_err("failed to set work dir")?;
        fsconfig(&fs_fd, FsconfigCommand::CmdCreate, Option::None, Option::None, 0).with_err("failed to create config command")?;
        let mnt_fd = fsmount(&fs_fd, 0, 0).with_err("failed to mount rootfs")?;

        let target: std::borrow::Cow<'_, str> = merged_dir.to_string_lossy();
        move_mount(Some(&mnt_fd), None, None, Some(&target), MOVE_MOUNT_F_EMPTY_PATH).with_err("failed to move rootfs to path")?;
    }

    match fork().with_err("failed to fork process to enter contianer with pid = 1")? {
        0 => { /* just continue */},
        container_pid => {
            eventfd_write(&controller.eventfd_setup_main, container_pid as u64)?;
            info!("intermediate ready to exit");
            return Ok(());
        }
    }

    setsid().with_err("failed to create new session")?;

    // https://man7.org/linux/man-pages/man2/pivot_root.2.html#EXAMPLES
    let old_root = merged_dir.join("old_root");
    env::set_current_dir(&merged_dir)?;
    fs::create_dir(&old_root)?;
    pivot_root(&merged_dir, &old_root)?;
    env::set_current_dir("/")?;
    chroot(".")?;

    trace!("creating standard paths");
    let proc_dir = create_if_not_exists("/proc".into(), 0o755)?;
    let flags = libc::MS_NOEXEC | libc::MS_NODEV | libc::MS_NOSUID;
    mount("proc", &proc_dir.to_string_lossy(), "proc", flags, None)
        .with_err("mount /proc failed")?;

    let dev_dir = create_if_not_exists("/dev".into(), 0o755)?;
    let flags = libc::MS_NOSUID | libc::MS_NOEXEC;
    mount("tmpfs", &dev_dir.to_string_lossy(), "tmpfs", flags, Some("mode=755,size=65536k"))
        .with_err("failed to mount /dev as tmpfs")?;

    // https://docs.kernel.org/filesystems/devpts.html
    // https://man7.org/linux/man-pages/man8/mount.8.html (see "Mount options for devpts")
    let devpts_dir = create_if_not_exists("/dev/pts".into(), 0o755)?;
    let flags = libc::MS_NOSUID | libc::MS_NOEXEC;
    mount("devpts", &devpts_dir.to_string_lossy(), "devpts", flags, Some("newinstance,ptmxmode=0666,mode=0620"))
        .with_err("failed to mount /dev/pts as devpts")?;

    symlink("pts/ptmx", "/dev/ptmx").with_err("failed to create /dev/ptmx symlink")?;

    if controller.has_terminal {
        let master = OpenOptions::new().read(true).write(true).open("/dev/ptmx").with_err("failed to create master terminal")?;
        grantpt(&master)?;
        unlockpt(&master)?;
        let slave = take_slave_from_fd(&master)?;

        send_fd(&controller.socket_terminal_fd_container_side, &master).with_err("failed to send master fd to controller")?;

        dup2(&slave, &0)?;
        dup2(&slave, &1)?;
        dup2(&slave, &2)?;

        // mknod("/dev/tty", 0o666 | libc::S_IFCHR, libc::makedev(5, 0)).with_err("failed to create /dev/tty")?;
        File::create("/dev/tty").with_err("failed to create /dev/tty")?;
        move_mount(Some(&controller.mount_points.dev_tty), None, None, Some("/dev/tty"), MOVE_MOUNT_F_EMPTY_PATH).with_err("failed to move /dev/tty")?;
        mem::drop(controller.mount_points.dev_tty);

        // master and slave was closed here
    }

    // The mode argument specifies both the file mode to use and the type
    // of node to be created.  It should be a combination (using bitwise OR)
    // https://man7.org/linux/man-pages/man3/makedev.3.html
    // devices list: https://www.kernel.org/doc/Documentation/admin-guide/devices.txt
    // but there is a BIG problem: mknod doesn't work well in a user namespace.
    // we need to steal the devices from the init-namespace.
    // https://lore.kernel.org/all/20180705155120.22102-1-christian@brauner.io/
    // mknod("/dev/null", 0o666 | libc::S_IFCHR, libc::makedev(1, 3)).with_err("failed to create /dev/null")?;

    // https://man7.org/linux/man-pages/man2/open_tree.2.html#EXAMPLES
    File::create("/dev/null").with_err("failed to create /dev/null")?;
    move_mount(Some(&controller.mount_points.dev_null), None, None, Some("/dev/null"), MOVE_MOUNT_F_EMPTY_PATH).with_err("failed to move /dev/null")?;
    mem::drop(controller.mount_points.dev_null);

    let working_dir = config.working_dir.as_ref().map(|s| if s.is_empty() { "/" } else { s }).unwrap_or(&"/");
    env::set_current_dir(working_dir).with_err("failed to set current dir to working directory")?;

    std::mem::drop(controller.eventfd_setup_main);
    std::mem::drop(controller.eventfd_setup_intermediate);

    let path = "/old_root";
    let mounts_file = "/proc/mounts";

    umount("/old_root", libc::MNT_DETACH).with_err("failed to unmount /old_root")?;
    // ensure that old_root is unmounted.
    // https://man7.org/linux/man-pages/man5/fstab.5.html
    let has_old_root = fs::read_to_string(mounts_file)
        .with_err("failed to read mount points")?
        .lines()
        .into_iter()
        .filter_map(|x| x.split_ascii_whitespace().nth(1))
        .any(|x| x.starts_with(path));
    if has_old_root {
        return Err("failed to umount detach the /old_root. starting the container will grant access to the host's file system.".into());
    }

    if args.restrict_syscalls {
        restrict_syscalls().with_err("failed to restrict syscalls")?;
    }

    let (cmd, process_args) = compile_cmd(&config)?;
    let envs: Vec<String> = config.env.unwrap_or_else(|| Vec::new());
    let envs = [args.envs, envs].concat();

    trace!("ready to exec!");
    trace!("exec: cmd: {}", cmd);
    trace!("exec: args: {:?}", process_args);
    trace!("exec: envs: {:?}", envs);
    execve(cmd, process_args, envs).with_err("faild to execute container")?;

    // imposible path because execve never return ok (it replcae the process)
    return Ok(());
}

fn microdocker_main_process(container_dir: PathBuf, intermediate_pid: i32, context: ControllerProcessContext) -> Result<(), String> {
    info!("container stored in: {}", &container_dir.to_string_lossy());

    trace!("setting up main process as child subreaper");
    prctl(libc::PR_SET_CHILD_SUBREAPER).with_err("failed to set main process as child subreaper")?;

    unsafe {
        let host_uid = getuid();
        let host_gid = getgid();

        trace!("wait for the intermediate process to unshare");
        eventfd_read(&context.eventfd_setup_main)?; 
        trace!("setting up uid/gid mapping");

        // abount uid/gid mappings: https://blog.quarkslab.com/digging-into-linux-namespaces-part-2.html
        // abount subordinate uid/gid: https://man7.org/linux/man-pages/man5/subuid.5.html
        let uid_map = find_sub_mapping("/etc/subuid");
        if let Err(e) = &uid_map {
            info!("skip: subordinate user id not found: {}", e);
        }
        let gid_map = find_sub_mapping("/etc/subgid");
        if let Err(e) = &gid_map {
            info!("skip: subordinate group id not found: {}", e);
        }
        if let Err(e) = apply_mapping("newuidmap", uid_map.ok(), intermediate_pid, host_uid) {
            info!("error while applying uid mapping: {}", e);
        }
        if let Err(e) = apply_mapping("newgidmap", gid_map.ok(), intermediate_pid, host_gid) {
            info!("error while applying gid mapping: {}", e);
        }
        trace!("continue the intermediate process");
        eventfd_write(&context.eventfd_setup_intermediate, 1)?;

        let container_pid = eventfd_read(&context.eventfd_setup_main)? as i32;
        info!("container process id: {}", container_pid);

        // let mut waitpid_task = tokio::task::spawn_blocking(move || {
        //     loop {
        //         let pid = waitpid(-1, 0); 
        //         match pid {
        //             Ok(result) if result.0 == container_pid => {
        //                 info!("container process exited");
        //                 break;
        //             },
        //             Ok(result) => {
        //                 trace!("pid {} exited", result.0)
        //             },
        //             Err(v) => {
        //                 info!("waitpid error: {}", v);
        //                 break;
        //             }
        //         }
        //     }
        // });

        if context.has_terminal {
            info!("wait for terminal");

            let master = recv_fd(&context.socket_terminal_fd_main_side).with_err("failed to receive master fd")?;
            info!("container terminal master received: {}", master.as_raw_fd());

            let ws = current_winsize();
            set_winsize(&master, &ws).with_err("failed to set window size")?;
            info!("terminal size: {}x{}", ws.ws_col, ws.ws_row);

            // switch the current terminal to raw mode to proxy all bytes as is
            // see "Raw mode"
            // https://man7.org/linux/man-pages/man3/termios.3.html
            let termios = TermiosState::make_raw().with_err("failed to switch host terminal to raw mode")?;

            // todo: need to reap zombies until container is alive
            proxy(master, container_pid).with_err("main process supervisor error")?;

            // restore terminal
            mem::drop(termios);

            info!("supervisor done its job, remove the container state");
        } else {
            // let _ = waitpid_task.await;
        }

        // todo: make container removal stable.
        if let Err(e) = env::set_current_dir(&container_dir) {
            return Err(format!("failed to change directory to: {:#?}, error: {}", &container_dir, e).into());
        }
        let work_dir = container_dir.join("work").join("work").into_os_string().into_string().unwrap();
        let c_work_dir = CString::new(work_dir).unwrap();
        if chmod(c_work_dir.as_ptr(), S_IRUSR | S_IWUSR | S_IXUSR) == -1 {
            info!("Warning: failed to chmod work/work: {}", std::io::Error::last_os_error());
        };
        if let Err(e) = fs::remove_dir_all(container_dir) {
            info!("Warning: failed to remove container directory: {}", e);
        }
        info!("container {} stopped", context.main_pid);
        return Ok(());
    }
}

fn ready(p: &pollfd) -> bool {
    p.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0
}

fn proxy(master: OwnedFd, child: Pid) -> Result<(), String> {
    let mut master = File::from(master);
    let mut stdin = ManuallyDrop::new(unsafe { File::from_raw_fd(io::stdin().as_raw_fd()) });
    let mut stdout = ManuallyDrop::new(unsafe { File::from_raw_fd(io::stdout().as_raw_fd()) });
    let pid = pidfd_open(&child).with_err("failed to open container pid as pidfd")?;
    let mut buffer = [0u8; 8192];

    let mut master_open = true;
    let mut stdin_open = true;
    let mut container_alive = true;
    loop {
        if !master_open && !container_alive {
            break;
        }
        let mut fds = [
            pollfd { fd: if master_open { master.as_raw_fd() } else { -1 }, events: libc::POLLIN, revents: 0 },
            pollfd { fd: if stdin_open { stdin.as_raw_fd() } else { -1 }, events: libc::POLLIN, revents: 0 },
            pollfd { fd: pid.as_raw_fd(), events: libc::POLLIN, revents: 0}
        ];
        let result = poll(&mut fds)?;
        match result {
            PollResult::TimeOut => continue,
            PollResult::Count(_) => {},
        }
        if ready(&fds[0]) {
            match master.read(&mut buffer) {
                Ok(0) => {
                    master_open = false;
                    trace!("container master was closed");
                },
                Ok(n) => {
                    stdout.write_all(&buffer[..n]).with_err("failed to write master -> stdout")?;
                    stdout.flush().with_err("failed to flush master -> stdout")?;
                },
                // EIO when slave closed
                // https://github.com/torvalds/linux/blob/v7.0/drivers/tty/n_tty.c#L2140-L2141
                Err(_) => {
                    master_open = false;
                    trace!("container master was closed");
                },
            }
        }
        if ready(&fds[1]) {
            match stdin.read(&mut buffer) {
                Ok(0) => {
                    stdin_open = false;
                    trace!("host stdin was closed");
                },
                Ok(n) => {
                    master.write_all(&buffer[..n]).with_err("failed to write stdin -> master")?;
                    master.flush().with_err("failed to flush stdin -> master")?;
                },
                Err(e) => {
                    e.downcast().with_err("error while reading the host stdin")?;
                },
            }
        }
        if ready(&fds[2]) {
            info!("container process exited");
            container_alive = false;
        }
    }

    Ok(())
}

fn parse_www_authenticate(header: &HeaderValue) -> HashMap<String, String> {
    let value: &str = header.to_str().unwrap();
    if !value.starts_with("Bearer") {
        panic!("failed to authenticate: only Bearer tokens are support");
    };

    value.split_once(' ')
        .expect("")
        .1
        .split(',')
        .map(|part| part.trim())
        .filter_map(|part| {
            let parts = part.split_once('=');
            if let Some(parts) = parts { Some((parts.0.to_string(), parts.1.trim_matches('"').to_string())) } else { None }
        })
        .collect()
}

async fn get(url: &str) -> Result<Response, Box<dyn std::error::Error>> {
    let client = Client::new();
    let mut response = client
        .get(url)
        // https://distribution.github.io/distribution/spec/manifest-v2-2/#backward-compatibility
        .header("Accept", MANIFEST_V2_SCHEMA)
        .header("Accept", MANIFEST_LIST_V2_SCHEMA)
        .send().await?;

    if response.status() == 401 {
    let headers = std::mem::take(response.headers_mut());
        let header = headers.get(WWW_AUTHENTICATE_HEADER_NAME).expect(&format!("{:#?} header is not present", WWW_AUTHENTICATE_HEADER_NAME));
        let values = parse_www_authenticate(header);
        let response = client
            .get(&values["realm"])
            .query(&[
                ("service", &values["service"]),
                ("scope", &values["scope"])
            ])
            .send()
            .await?
            .json::<TokenResponse>()
            .await?;

        return Ok(client
            .get(url)
            .header("Accept", MANIFEST_V2_SCHEMA)
            .header("Accept", MANIFEST_LIST_V2_SCHEMA)
            .bearer_auth(response.token)
            .send()
            .await?);
    }

    Ok(response)
}

async fn registry_get(reference: &SimpleReference, content: &str) -> Result<Response, Box<dyn std::error::Error>> {
    let base = PathBuf::from(DEFAULT_REGISTRY_ADDRESS);
    let url: String = base
        .join("v2")
        .join(&reference.path)
        .join(content)
        .join(reference.tag())
        .to_str()
        .unwrap()
        .to_string();

    get(&url).await
}

// https://distribution.github.io/distribution/spec/api/#pulling-an-image-manifest
async fn get_manifest(reference: &SimpleReference) -> Result<Manifest, Box<dyn Error>> {
    let response = registry_get(reference, "manifests").await?;
    let content_type = response.headers().get("Content-Type").unwrap().to_str()?;

    if content_type == INDEX_SCHEMA {
        let index = response.json::<Index>().await?;
        let digest = index.manifests.first().expect("there is no manifests").digest.as_ref().unwrap();
        return Box::pin(get_manifest(&reference.with_hash(&digest))).await;
    }

    let result = Box::pin(response.json::<Manifest>()).await?;
    return Ok(result);
}

// https://distribution.github.io/distribution/spec/api/#pulling-a-layer
async fn get_blob(reference: &SimpleReference) -> Result<Bytes, Box<dyn Error>> {
    Ok(registry_get(reference, "blobs").await?.bytes().await?)
}

async fn download_layers(reference: &SimpleReference, layers: &Vec<Descriptor>) -> Result<PackedLayers, Box<dyn Error>> {
    let packed = PackedLayers::new()?;
    for layer in layers {
        let digest = match &layer.digest {
            Some(value) => value,
            None => return Err("digest is omited".into())
        };
        info!("pulling the layer: {}, size: {:#?}", digest, layer.size.unwrap_or_default());

        let body = get_blob(&reference.with_hash(&digest)).await.expect("failed to fetch blob");
        let tar_path = packed.location.join(&digest);
        let mut tar = File::create(&tar_path).expect("failed to create tar file");
        let mut gzip = GzDecoder::new(BufReader::new(body.as_ref()));
        io::copy(&mut gzip, &mut tar).expect("failed to extract layer");
    }

    Ok(packed)
}

async fn unpack_layers(source: PackedLayers, destination: &PathBuf) {
    for source in fs::read_dir(&source.location).expect("failed to read temp dir") {
        if let Ok(entry) = source {
            let mut tar = File::open(&entry.path()).unwrap();
            let mut hasher = Sha256::new();
            io::copy(&mut tar, &mut hasher).unwrap();
            let hash = format!("{:x}", hasher.finalize());
            let destination = destination.join(hash);

            tar.seek(SeekFrom::Start(0)).unwrap();
            let mut archive = Archive::new(tar);
            archive.unpack(destination).unwrap();
        }
    }
}

// https://docs.docker.com/reference/dockerfile/#understand-how-cmd-and-entrypoint-interact
fn compile_cmd(config: &ImageConfig) -> Result<(String, Vec<String>), Box<dyn Error>> {
    match &config.entrypoint {
        Some(entrypoint) => {
            match &config.cmd {
                Some(cmd) => {
                    let mut iter = entrypoint.into_iter();

                    let command = match iter.next() {
                        Some(v) => v,
                        None => return Err("cmd is set but empty".into())
                    };

                    let mut entry = entrypoint.into_iter().map(|x| x.to_owned()).collect::<Vec<String>>();
                    entry.extend_from_slice(&cmd);
                    return Ok((command.to_owned(), entry))
                },
                None => {
                    let mut iter = entrypoint.into_iter();

                    let cmd = match iter.next() {
                        Some(v) => v,
                        None => return Err("cmd is set but empty".into())
                    };

                    return Ok((cmd.to_owned(), entrypoint.into_iter().map(|x| x.to_owned()).collect::<Vec<String>>()))
                }
            }
        },
        None => {
            match &config.cmd {
                Some(cmd) => {
                    let mut iter = cmd.into_iter();

                    let command = match iter.next() {
                        Some(v) => v,
                        None => return Err("cmd is set but empty".into())
                    };

                    return Ok((command.to_owned(), cmd.into_iter().map(|x| x.to_owned()).collect::<Vec<String>>()))
                },
                None => {
                    return Err("not allowed: entrypoint and cmd isn't set".into())
                }
            }
        }
    }
}

fn find_sub_mapping(filename: &str) -> Result<(u32, u32), String> {
    let uid = getuid();
    let uid_formatted = format!("{}", uid);
    let name = get_username(uid).with_err("failed to get username")?;
    trace!("finding subordinate in: {filename}, for uid: {uid} or name: {:?}", name);

    let file = fs::read_to_string(filename).with_err(&format!("failed to read: {}", &filename))?;
    for line in file.lines() {
        let array = line.split(':').collect::<Vec<&str>>();
        if array.len() != 3 {
            continue;
        }
        let user = array[0];
        if user != uid_formatted && name.as_ref().map_or(false, |n| n != user){
            continue;
        }
        let lower = array[1].parse::<u32>().unwrap();
        let count = array[2].parse::<u32>().unwrap();

        trace!("found subordinate: ({}, {})", lower, count);
        return Ok((lower, count));
    }

    return Err("mapping not found".to_owned());
}

fn apply_mapping(program: &str, sub: Option<(u32, u32)>, intermediate_pid: i32, host_id: u32) -> Result<Output, String> {
    let mut command = Command::new(program);
    command
        .arg(intermediate_pid.to_string())
        .arg("0").arg(host_id.to_string()).arg("1");
    return (match sub {
        Some(sub) => command.arg("1").arg(sub.0.to_string()).arg(sub.1.to_string()).output(),
        None => command.output(),
    }).with_err(&format!("failed to call '{}'", program));
}

struct PackedLayers {
    location: PathBuf
}

impl PackedLayers {
    pub fn new() -> Result<PackedLayers, Box<dyn Error>> {
        // you can generate random path instead of 'microdocker-pull' to parallel pulling images
        let temp_dir = env::temp_dir().join("microdocker-pull");
        if fs::read_dir(&temp_dir).is_ok() {
            return Err(format!("path {:#?} already exists, maybe another instance of microdocker is pulling images?", &temp_dir).into());
        }
        fs::create_dir_all(&temp_dir)?;

        Ok(PackedLayers { location: temp_dir })
    }
}

impl Drop for PackedLayers {
    fn drop(&mut self) {
        if let Err(err) = fs::remove_dir_all(&self.location) {
            info!("failed to remove location: {}, error: {}", self.location.display(), err);
        }
    }
}

struct SpecDigest {
    alg: String,
    hash: String
}

impl SpecDigest {
    pub fn parse(value: &str) -> Result<SpecDigest, Box<dyn Error>> {
        if let Some((alg, hash)) = value.split_once(':') {
            return Ok(SpecDigest { alg: alg.to_owned(), hash: hash.to_owned() });
        }

        return Err(format!("unable to parse digest: {}", value).into());
    }
}

#[derive(Deserialize, Debug)]
struct TokenResponse {
    token: String
}

struct ControllerProcessContext {
    eventfd_setup_main: OwnedFd,
    eventfd_setup_intermediate: OwnedFd,
    socket_terminal_fd_main_side: OwnedFd,
    socket_terminal_fd_container_side: OwnedFd,
    main_pid: Pid,
    has_terminal: bool,
    mount_points: DetachedMountPoints
}

struct DetachedMountPoints {
    dev_tty: OwnedFd,
    dev_null: OwnedFd
}