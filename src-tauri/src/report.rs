use std::path::PathBuf;

use crate::logging;

/// Crash-report destination. Plus-addressing keeps reports filterable in the
/// inbox.
pub const REPORT_EMAIL: &str = "akhlasahmed.aka+blinkwell@gmail.com";

/// Open the user's mail app with a prefilled message and the log files
/// attached. Windows goes through Simple MAPI, Linux through xdg-email; the
/// error is technical and meant for the log - the UI shows friendly fallback
/// guidance instead.
pub fn compose_report(subject: &str, body: &str, files: &[PathBuf]) -> Result<(), String> {
    if files.is_empty() {
        return Err("there are no log files to send yet".to_string());
    }
    platform_compose(subject, body, files)
}

/// Copy the log files to a temp folder and return the copy paths. Attaching
/// copies means the mail client never has to open files this app keeps open
/// for writing, and the originals are left untouched no matter what the
/// client does with the attachments.
pub fn prepare_attachments(files: &[PathBuf]) -> Vec<PathBuf> {
    let dir = std::env::temp_dir().join("blinkwell-report");
    if std::fs::create_dir_all(&dir).is_err() {
        return files.to_vec();
    }
    let mut attachments = Vec::new();
    for file in files {
        let Some(name) = file.file_name() else {
            continue;
        };
        let target = dir.join(name);
        match std::fs::copy(file, &target) {
            Ok(_) => attachments.push(target),
            // If a copy fails, the original path is still a usable fallback.
            Err(_) => attachments.push(file.clone()),
        }
    }
    if attachments.is_empty() {
        files.to_vec()
    } else {
        attachments
    }
}

/// Fallback path when no mail app exists: open a prefilled Gmail draft in the
/// browser. The logs reach the draft through the clipboard (see
/// `copy_files_to_clipboard`), so the user only has to paste and send.
pub fn open_web_report(subject: &str, body: &str) -> Result<(), String> {
    let url = format!(
        "https://mail.google.com/mail/?view=cm&fs=1&to={}&su={}&body={}",
        url_encode(REPORT_EMAIL),
        url_encode(subject),
        url_encode(body)
    );
    platform_open_url(&url)
}

/// Put the log files on the clipboard as a file list, exactly what Explorer's
/// Copy produces, so Ctrl+V in a browser mail draft attaches them.
pub fn copy_files_to_clipboard(files: &[PathBuf]) -> Result<(), String> {
    if files.is_empty() {
        return Err("there are no files to copy".to_string());
    }
    platform_clipboard_files(files)
}

/// Percent-encode for a URL query. Only unreserved characters survive.
fn url_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }
    out
}

#[cfg(windows)]
#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteW(
        hwnd: *mut core::ffi::c_void,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
    ) -> *mut core::ffi::c_void;
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn OpenClipboard(owner: *mut core::ffi::c_void) -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, data: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn CloseClipboard() -> i32;
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> *mut core::ffi::c_void;
    fn GlobalLock(block: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn GlobalUnlock(block: *mut core::ffi::c_void) -> i32;
    fn GlobalFree(block: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
}

#[cfg(windows)]
fn platform_open_url(url: &str) -> Result<(), String> {
    // ShellExecuteW hands the URL to the shell, which opens it in the
    // default browser - the same path as double-clicking a link.
    let operation = wide("open");
    let target = wide(url);
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if result as isize > 32 {
        Ok(())
    } else {
        Err(format!("ShellExecuteW returned {}", result as isize))
    }
}

#[cfg(not(windows))]
fn platform_open_url(url: &str) -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("could not start xdg-open: {err}"))
}

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Put the log files on the clipboard as CF_HDROP, the same format Explorer
/// uses for Copy. Browsers expose it to paste handlers, so Ctrl+V in a Gmail
/// draft attaches the files.
#[cfg(windows)]
fn platform_clipboard_files(files: &[PathBuf]) -> Result<(), String> {
    const CF_HDROP: u32 = 15;
    const GMEM_MOVEABLE: u32 = 0x0002;
    // DROPFILES from shellapi.h is 20 bytes: the offset where the path list
    // starts, the mouse position, a non-client flag and a unicode flag.
    const HEADER: usize = 20;

    let mut list: Vec<u16> = Vec::new();
    for file in files {
        list.extend(file.to_string_lossy().encode_utf16());
        list.push(0);
    }
    // One terminator per path, plus the final one for the whole list.
    list.push(0);
    let list_bytes = list.len() * std::mem::size_of::<u16>();
    let total = HEADER + list_bytes;

    let mut header = [0u8; HEADER];
    header[0..4].copy_from_slice(&(HEADER as u32).to_le_bytes()); // pFiles
    header[16..20].copy_from_slice(&1i32.to_le_bytes()); // fWide = TRUE

    // Another app can hold the clipboard briefly; give it a few tries.
    let mut opened = false;
    for _ in 0..10 {
        if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
            opened = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if !opened {
        return Err("the clipboard is busy".to_string());
    }

    let result = unsafe {
        let block = GlobalAlloc(GMEM_MOVEABLE, total);
        if block.is_null() {
            Err("clipboard memory could not be allocated".to_string())
        } else {
            let locked = GlobalLock(block);
            if locked.is_null() {
                GlobalFree(block);
                Err("clipboard memory could not be locked".to_string())
            } else {
                std::ptr::copy_nonoverlapping(header.as_ptr(), locked.cast::<u8>(), HEADER);
                std::ptr::copy_nonoverlapping(
                    list.as_ptr().cast::<u8>(),
                    locked.cast::<u8>().add(HEADER),
                    list_bytes,
                );
                GlobalUnlock(block);

                if EmptyClipboard() == 0 {
                    GlobalFree(block);
                    Err("the clipboard could not be cleared".to_string())
                } else if SetClipboardData(CF_HDROP, block).is_null() {
                    GlobalFree(block);
                    Err("the clipboard did not accept the files".to_string())
                } else {
                    // On success the system owns the block; freeing it here
                    // would corrupt the clipboard.
                    Ok(())
                }
            }
        }
    };
    unsafe { CloseClipboard() };
    result
}

#[cfg(not(windows))]
fn platform_clipboard_files(_files: &[PathBuf]) -> Result<(), String> {
    Err("clipboard file copy is not implemented on this platform".to_string())
}

/// Open the logs folder in the file manager (with the newest log file
/// selected, where the platform supports it) and return the folder path.
pub fn reveal_logs_folder() -> Result<PathBuf, String> {
    let dir = logging::dir().ok_or_else(|| "the logs folder is not available yet".to_string())?;
    let _ = std::fs::create_dir_all(&dir);
    let newest = logging::recent_files(1).into_iter().next();
    platform_reveal(&dir, newest.as_deref())?;
    Ok(dir)
}

#[cfg(windows)]
fn platform_compose(subject: &str, body: &str, files: &[PathBuf]) -> Result<(), String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{channel, RecvTimeoutError};
    use std::time::Duration;

    // A compose request that never comes back keeps its thread blocked. The
    // timeout keeps the UI honest, and the flag stops a second click from
    // stacking another stuck request on top of the first.
    static MAPI_BUSY: AtomicBool = AtomicBool::new(false);
    const MAPI_TIMEOUT: Duration = Duration::from_secs(20);

    // On a PC without a registered mail client, Simple MAPI does not fail
    // fast - it can block for minutes. Checking first keeps that call off
    // machines that could never answer it anyway.
    if mapi::default_client_name().is_none() {
        return Err("no default email app is set up on this PC".to_string());
    }

    if MAPI_BUSY.swap(true, Ordering::SeqCst) {
        return Err("a previous report is still waiting on your email app".to_string());
    }

    let subject = subject.to_string();
    let body = body.to_string();
    let files = files.to_vec();
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("blinkwell-mapi".to_string())
        .spawn(move || {
            let _ = tx.send(mapi::compose(&subject, &body, &files));
        })
        .map_err(|err| {
            MAPI_BUSY.store(false, Ordering::SeqCst);
            format!("could not start the mail task: {err}")
        })?;

    match rx.recv_timeout(MAPI_TIMEOUT) {
        Ok(result) => {
            MAPI_BUSY.store(false, Ordering::SeqCst);
            result
        }
        // The detached thread may still finish later; the flag stays set so a
        // second click falls through to the manual path instead.
        Err(RecvTimeoutError::Timeout) => Err("the email app did not answer in time".to_string()),
        Err(RecvTimeoutError::Disconnected) => {
            MAPI_BUSY.store(false, Ordering::SeqCst);
            Err("the mail task ended unexpectedly".to_string())
        }
    }
}

#[cfg(not(windows))]
fn platform_compose(subject: &str, _body: &str, files: &[PathBuf]) -> Result<(), String> {
    let mut command = std::process::Command::new("xdg-email");
    command.arg("--subject").arg(subject);
    for file in files {
        command.arg("--attach").arg(file);
    }
    command.arg(format!("mailto:{REPORT_EMAIL}"));

    let status = command
        .status()
        .map_err(|err| format!("could not start a mail client: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("the mail client did not open".to_string())
    }
}

#[cfg(windows)]
fn platform_reveal(dir: &std::path::Path, newest: Option<&std::path::Path>) -> Result<(), String> {
    if let Some(file) = newest {
        // Open the folder with the newest log file already highlighted, so
        // the manual attach is a drag away.
        let selected = std::process::Command::new("explorer")
            .arg(format!("/select,{}", file.display()))
            .spawn();
        if selected.is_ok() {
            return Ok(());
        }
    }
    std::process::Command::new("explorer")
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("could not open the logs folder: {err}"))
}

#[cfg(not(windows))]
fn platform_reveal(dir: &std::path::Path, _newest: Option<&std::path::Path>) -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("could not open the logs folder: {err}"))
}

/// Simple MAPI bindings. `MAPISendMailW` hands the message to the default
/// mail client, which opens its compose window with the attachments filled
/// in; the user reviews and sends it themselves, so no mail credentials ever
/// live inside the app.
///
/// The Windows SDK import library only ships the ANSI entry point, so the
/// Unicode one is resolved at runtime, with a graceful error when it is
/// missing.
#[cfg(windows)]
mod mapi {
    use std::path::PathBuf;
    use std::ptr;

    use super::{wide, REPORT_EMAIL};

    const MAPI_LOGON_UI: u32 = 0x0000_0001;
    const MAPI_DIALOG: u32 = 0x0000_0008;
    const MAPI_TO: u32 = 1;
    const SUCCESS: u32 = 0;
    const COINIT_APARTMENTTHREADED: u32 = 0x2;

    // Predefined registry roots, sign-extended the way the Win32 headers do.
    const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as i32 as isize;
    const HKEY_LOCAL_MACHINE: isize = 0x8000_0002u32 as i32 as isize;
    /// RRF_RT_REG_SZ: accept only REG_SZ values.
    const RRF_RT_REG_SZ: u32 = 0x0000_0002;
    const ERROR_SUCCESS: i32 = 0;

    #[repr(C)]
    struct RecipW {
        reserved: u32,
        recip_class: u32,
        name: *const u16,
        address: *const u16,
        eid_size: u32,
        eid: *mut core::ffi::c_void,
    }

    #[repr(C)]
    struct FileW {
        reserved: u32,
        flags: u32,
        position: u32,
        path: *const u16,
        name: *const u16,
        file_type: *mut core::ffi::c_void,
    }

    #[repr(C)]
    struct MessageW {
        reserved: u32,
        subject: *const u16,
        note: *const u16,
        message_type: *const u16,
        date_received: *const u16,
        conversation: *const u16,
        flags: u32,
        originator: *mut core::ffi::c_void,
        recipients: *mut RecipW,
        recipient_count: u32,
        files: *mut FileW,
        file_count: u32,
    }

    type SendMailW = unsafe extern "system" fn(usize, usize, *const MessageW, u32, u32) -> u32;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut core::ffi::c_void;
        fn GetProcAddress(module: *mut core::ffi::c_void, name: *const u8)
            -> *mut core::ffi::c_void;
        fn FreeLibrary(module: *mut core::ffi::c_void) -> i32;
    }

    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(reserved: *mut core::ffi::c_void, coinit: u32) -> i32;
        fn CoUninitialize();
    }

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(
            hkey: isize,
            subkey: *const u16,
            value: *const u16,
            flags: u32,
            value_type: *mut u32,
            data: *mut core::ffi::c_void,
            data_size: *mut u32,
        ) -> i32;
    }

    /// The registered default mail client, if the system has one. Absence is
    /// worth knowing: Simple MAPI blocks instead of failing fast without a
    /// client, so nothing is attempted in that case.
    pub fn default_client_name() -> Option<String> {
        [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
            .into_iter()
            .find_map(read_default_client)
    }

    fn read_default_client(hive: isize) -> Option<String> {
        let subkey = wide("SOFTWARE\\Clients\\Mail");
        let mut buffer = [0u16; 512];
        let mut size = (buffer.len() * std::mem::size_of::<u16>()) as u32;
        let status = unsafe {
            RegGetValueW(
                hive,
                subkey.as_ptr(),
                ptr::null(),
                RRF_RT_REG_SZ,
                ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS || size < 2 {
            return None;
        }
        // `size` includes the terminating null.
        let length = ((size as usize / 2) - 1).min(buffer.len());
        let name = String::from_utf16_lossy(&buffer[..length]);
        let name = name.trim();
        if name.is_empty() {
            None
        } else {
            Some(name.to_string())
        }
    }

    pub fn compose(subject: &str, body: &str, files: &[PathBuf]) -> Result<(), String> {
        let library_name = wide("mapi32.dll");
        let module = unsafe { LoadLibraryW(library_name.as_ptr()) };
        if module.is_null() {
            return Err("the Windows mail interface is not available".to_string());
        }

        let outcome = match send_mail_pointer(module) {
            Some(send_mail) => compose_with(send_mail, subject, body, files),
            None => Err("this Windows version has no Unicode mail interface".to_string()),
        };
        unsafe { FreeLibrary(module) };
        outcome
    }

    fn send_mail_pointer(module: *mut core::ffi::c_void) -> Option<SendMailW> {
        static NAME: &[u8] = b"MAPISendMailW\0";
        let pointer = unsafe { GetProcAddress(module, NAME.as_ptr()) };
        if pointer.is_null() {
            None
        } else {
            Some(unsafe { std::mem::transmute::<*mut core::ffi::c_void, SendMailW>(pointer) })
        }
    }

    fn compose_with(
        send_mail: SendMailW,
        subject: &str,
        body: &str,
        files: &[PathBuf],
    ) -> Result<(), String> {
        let subject_w = wide(subject);
        let body_w = wide(body);
        let address_w = wide(REPORT_EMAIL);

        let file_strings: Vec<(Vec<u16>, Vec<u16>)> = files
            .iter()
            .filter_map(|path| {
                let name = path.file_name()?.to_string_lossy().into_owned();
                Some((wide(&path.to_string_lossy()), wide(&name)))
            })
            .collect();

        // Every buffer must stay alive until MAPISendMailW returns.
        let mut file_descs: Vec<FileW> = file_strings
            .iter()
            .map(|(path, name)| FileW {
                reserved: 0,
                flags: 0,
                position: u32::MAX,
                path: path.as_ptr(),
                name: name.as_ptr(),
                file_type: ptr::null_mut(),
            })
            .collect();

        let mut recipients = [RecipW {
            reserved: 0,
            recip_class: MAPI_TO,
            name: ptr::null(),
            address: address_w.as_ptr(),
            eid_size: 0,
            eid: ptr::null_mut(),
        }];

        let files_ptr = if file_descs.is_empty() {
            ptr::null_mut()
        } else {
            file_descs.as_mut_ptr()
        };

        let message = MessageW {
            reserved: 0,
            subject: subject_w.as_ptr(),
            note: body_w.as_ptr(),
            message_type: ptr::null(),
            date_received: ptr::null(),
            conversation: ptr::null(),
            flags: 0,
            originator: ptr::null_mut(),
            recipients: recipients.as_mut_ptr(),
            recipient_count: 1,
            files: files_ptr,
            file_count: file_descs.len() as u32,
        };

        // The mail client wants a COM-initialised caller thread; failures are
        // tolerated (an already-initialised thread is fine).
        let com = unsafe { CoInitializeEx(ptr::null_mut(), COINIT_APARTMENTTHREADED) };
        let result = unsafe { send_mail(0, 0, &message, MAPI_LOGON_UI | MAPI_DIALOG, 0) };
        if com >= 0 {
            unsafe { CoUninitialize() };
        }

        if result == SUCCESS {
            Ok(())
        } else {
            Err(format!("the mail client returned MAPI error {result}"))
        }
    }
}
