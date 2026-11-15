//! Single instance: a second launch (e.g. opening a PDF from the file
//! manager) hands its files to the running Leme and exits, so they open
//! as tabs in the current session.
//!
//! The channel is local only: a Unix socket in the user's runtime folder
//! on Linux, a named pipe on Windows. A message is the absolute paths, one
//! per line; an empty message just brings the window forward.

use std::io::{self, Read, Write};
use std::path::PathBuf;

/// Debug builds use their own channel, so `cargo run` never hands its
/// files to an installed Leme.
const NAME: &str = if cfg!(debug_assertions) {
    "leme-dev"
} else {
    "leme"
};

/// Sends `paths` to a running instance. True if one received them, in
/// which case this process should exit.
pub fn forward(paths: &[PathBuf]) -> bool {
    let message = encode(paths);
    match connect() {
        Ok(mut stream) => {
            allow_focus();
            stream.write_all(message.as_bytes()).is_ok()
        }
        Err(_) => false,
    }
}

/// Listens for later launches on a background thread, calling `on_open`
/// with each one's files. Blocks in the OS between messages; no polling.
pub fn listen(on_open: impl Fn(Vec<PathBuf>) + Send + 'static) -> io::Result<()> {
    let server = Server::bind()?;
    std::thread::Builder::new()
        .name("instance".into())
        .spawn(move || loop {
            match server.accept() {
                Ok(mut stream) => {
                    let mut text = String::new();
                    // Bounded: only small path lists are expected.
                    if (&mut stream)
                        .take(1 << 20)
                        .read_to_string(&mut text)
                        .is_ok()
                    {
                        on_open(decode(&text));
                    }
                }
                Err(_) => return,
            }
        })?;
    Ok(())
}

fn encode(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .filter_map(|p| std::path::absolute(p).ok())
        // A path with a newline or invalid UTF-8 can't be framed; the
        // user can still open it from the dialog.
        .filter_map(|p| p.to_str().map(str::to_owned))
        .filter(|p| !p.contains('\n'))
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .collect()
}

#[cfg(unix)]
use unix::{allow_focus, connect, Server};
#[cfg(windows)]
use windows::{allow_focus, connect, Server};

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;

    fn socket_path() -> Option<PathBuf> {
        // XDG_RUNTIME_DIR is private to the user (0700).
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(crate::config::cache_dir)?;
        Some(dir.join(format!("{NAME}.sock")))
    }

    pub fn connect() -> io::Result<UnixStream> {
        UnixStream::connect(socket_path().ok_or_else(no_dir)?)
    }

    pub fn allow_focus() {}

    pub struct Server(UnixListener);

    impl Server {
        pub fn bind() -> io::Result<Self> {
            let path = socket_path().ok_or_else(no_dir)?;
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            // No one answered `forward`, so a leftover socket is stale.
            remove_stale(&path);
            UnixListener::bind(&path).map(Self)
        }

        pub fn accept(&self) -> io::Result<UnixStream> {
            self.0.accept().map(|(stream, _)| stream)
        }
    }

    fn remove_stale(path: &Path) {
        if UnixStream::connect(path).is_err() {
            let _ = std::fs::remove_file(path);
        }
    }

    fn no_dir() -> io::Error {
        io::Error::other("no runtime folder")
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::fs::{File, OpenOptions};
    use std::os::windows::io::FromRawHandle;

    const PIPE_ACCESS_INBOUND: u32 = 0x1;
    const FILE_FLAG_FIRST_PIPE_INSTANCE: u32 = 0x0008_0000;
    const PIPE_REJECT_REMOTE_CLIENTS: u32 = 0x8;
    const PIPE_UNLIMITED_INSTANCES: u32 = 255;
    const ERROR_PIPE_CONNECTED: i32 = 535;
    const INVALID_HANDLE_VALUE: isize = -1;
    const ASFW_ANY: u32 = u32::MAX;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateNamedPipeW(
            name: *const u16,
            open_mode: u32,
            pipe_mode: u32,
            max_instances: u32,
            out_buffer: u32,
            in_buffer: u32,
            timeout: u32,
            security: *mut core::ffi::c_void,
        ) -> isize;
        fn ConnectNamedPipe(pipe: isize, overlapped: *mut core::ffi::c_void) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }

    #[link(name = "user32")]
    extern "system" {
        fn AllowSetForegroundWindow(process_id: u32) -> i32;
    }

    /// Per user, since the pipe namespace is shared by every session.
    fn pipe_name() -> String {
        let user = std::env::var("USERNAME").unwrap_or_default();
        format!(r"\\.\pipe\{NAME}-{user}")
    }

    pub fn connect() -> io::Result<File> {
        OpenOptions::new().write(true).open(pipe_name())
    }

    /// Lets the running instance bring its window to the front; Windows
    /// otherwise only flashes it in the taskbar.
    pub fn allow_focus() {
        // SAFETY: plain call with a constant; failure only means no focus.
        unsafe {
            AllowSetForegroundWindow(ASFW_ANY);
        }
    }

    pub struct Server {
        name: Vec<u16>,
        // The first instance, created in `bind` so a second Leme racing to
        // start fails there instead of sharing the pipe.
        first: std::sync::Mutex<Option<isize>>,
    }

    impl Server {
        pub fn bind() -> io::Result<Self> {
            let name: Vec<u16> = pipe_name().encode_utf16().chain([0]).collect();
            let first = create(&name, FILE_FLAG_FIRST_PIPE_INSTANCE)?;
            Ok(Self {
                name,
                first: std::sync::Mutex::new(Some(first)),
            })
        }

        pub fn accept(&self) -> io::Result<File> {
            let first = self.first.lock().ok().and_then(|mut f| f.take());
            let pipe = match first {
                Some(pipe) => pipe,
                None => create(&self.name, 0)?,
            };
            // SAFETY: `pipe` is a valid pipe handle we own; a null
            // OVERLAPPED makes the call block until a client connects.
            let connected = unsafe { ConnectNamedPipe(pipe, core::ptr::null_mut()) } != 0;
            if !connected && io::Error::last_os_error().raw_os_error() != Some(ERROR_PIPE_CONNECTED)
            {
                // SAFETY: we own `pipe` and don't use it after closing.
                unsafe { CloseHandle(pipe) };
                return Err(io::Error::last_os_error());
            }
            // SAFETY: `pipe` is a valid handle we own; File closes it.
            Ok(unsafe { File::from_raw_handle(pipe as _) })
        }
    }

    fn create(name: &[u16], flags: u32) -> io::Result<isize> {
        // SAFETY: `name` is NUL-terminated UTF-16; a null security
        // descriptor gives the default, which lets only the owner write.
        let pipe = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_INBOUND | flags,
                PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                0,
                4096,
                0,
                core::ptr::null_mut(),
            )
        };
        if pipe == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(pipe)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_round_trip() {
        let dir = std::env::temp_dir();
        let paths = vec![dir.join("a b.pdf"), dir.join("ção.pdf")];
        assert_eq!(decode(&encode(&paths)), paths);
    }

    #[test]
    fn relative_paths_become_absolute() {
        let decoded = decode(&encode(&[PathBuf::from("doc.pdf")]));
        assert_eq!(decoded.len(), 1);
        assert!(decoded[0].is_absolute());
    }

    #[test]
    fn empty_message_means_focus_only() {
        assert!(decode(&encode(&[])).is_empty());
        assert!(decode("relative.pdf\n\n").is_empty());
    }
}
