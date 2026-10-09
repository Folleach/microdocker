# microdocker

A simple container engine implementation written for demonstration purposes.  
Similar to Docker and Podman.  
For a full-featured implementation, refer to the [runc](https://github.com/opencontainers/runc) repository

Written in `Rust`, `Linux`-only.

> [!WARNING]  
> The repository work in progress.  
> Don't reference it for now, full refactoring is still possible

# Features

- fetches the image and its layers from the [registry](https://github.com/distribution/distribution)
- combines the layers using the [overlay driver](https://docs.kernel.org/filesystems/overlayfs.html) with [fsconfig](https://man7.org/linux/man-pages/man2/fsconfig.2.html)
- runs without root ([rootless](https://rootlesscontaine.rs))
- isolates **{ user, pid, mount }** namespaces and does not touch **{ network, cgroup, time, uts, ipc }**
- creates its own terminal for the container, so you can run interactive commands like `vi` and `htop`
- restrict some syscalls:  
  `chdir` and `chroot` fail with `EPIPE` (broken pipe) when `--restrict-syscalls` is set

# How to run

You need the [Rust language](https://rust-lang.org/learn/get-started/) installed in your Linux environment.

```bash
cargo build -r
./target/release/microdocker run library/fedora
```

If you are running in a container, make sure your `$HOME/.local` directory is not in the overlay filesystem,  
because Linux does not allow creating overlay on top of another overlay.
```bash
mount -t tmpfs tmpfs $HOME/.local/share/microdocker
```
