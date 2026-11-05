//! Windows attachment safety primitives (F07, ADR 0026), behind safe functions.
//!
//! This is the workspace's only crate that may contain `unsafe` code: the COM and Win32 calls
//! below. Each unsafe block states why it is sound. Everything else (policy, re-verification,
//! durable state) lives in `dm-platform` and `dm-service` under the workspace's `forbid`.
//!
//! On other operating systems the functions exist and report "not supported", so callers need no
//! platform conditionals.

use std::path::Path;

/// `INET_E_SECURITY_PROBLEM`: Attachment Services refused the file by policy.
pub const INET_E_SECURITY_PROBLEM: i32 = 0x800C_000E_u32 as i32;

/// Downova's client GUID for Attachment Services (`SetClientGuid`): fixed, so per-client prompt
/// state is stable. Generated once for this project.
pub const DOWNOVA_CLIENT_GUID: u128 = 0x6d2f_4b1e_8c3a_4f57_9e21_b0d4_7a65_c381;

/// What `IAttachmentExecute::Save` returned. Never a malware verdict (the caller re-verifies the
/// file and decides).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveResult {
    /// `S_OK`.
    Saved,
    /// `INET_E_SECURITY_PROBLEM`.
    PolicyBlocked,
    /// Any other `HRESULT` (creation, apartment or call failure).
    Failed(i32),
    /// Not Windows.
    NotSupported,
}

/// Runs Windows Attachment Services on the file at `path` with `source` (an already sanitized
/// origin such as `https://host/`). Initializes COM as a single-threaded apartment **on the calling
/// thread** and uninitializes it before returning, so the caller must give it a thread of its own
/// (it blocks for as long as the registered trust services take).
pub fn save(path: &Path, source: &str) -> SaveResult {
    imp::save(path, source)
}

/// Whether the volume holding `path` supports named streams (`FILE_NAMED_STREAMS`); `None` when
/// it cannot be determined or this is not Windows.
pub fn named_streams_supported(path: &Path) -> Option<bool> {
    imp::named_streams_supported(path)
}

/// A file's identity independent of its name: volume serial number and file index (Windows).
/// `None` on other systems (the caller uses device and inode there).
pub fn file_identity(path: &Path) -> std::io::Result<Option<String>> {
    imp::file_identity(path)
}

#[cfg(windows)]
mod imp {
    use super::{DOWNOVA_CLIENT_GUID, INET_E_SECURITY_PROBLEM, SaveResult};
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle, GetVolumeInformationW,
        GetVolumePathNameW,
    };
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows::Win32::UI::Shell::{AttachmentServices, IAttachmentExecute};
    use windows::core::{GUID, HSTRING};

    /// `FILE_NAMED_STREAMS` (winnt.h).
    const FILE_NAMED_STREAMS: u32 = 0x0004_0000;

    /// Balances a successful `CoInitializeEx` on this thread.
    struct Apartment;
    impl Drop for Apartment {
        #[allow(unsafe_code)]
        fn drop(&mut self) {
            // SAFETY: created only after CoInitializeEx succeeded (S_OK or S_FALSE) on this same
            // thread; every COM object of this apartment was dropped before (see `save`).
            unsafe { CoUninitialize() };
        }
    }

    #[allow(unsafe_code)]
    pub(super) fn save(path: &Path, source: &str) -> SaveResult {
        // SAFETY: no reserved pointer; the apartment is initialized for this thread only and
        // balanced by `Apartment::drop` when it succeeded.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if hr.is_err() {
            // Includes RPC_E_CHANGED_MODE (the thread is already multithreaded): nothing to undo.
            return SaveResult::Failed(hr.0);
        }
        let _apartment = Apartment;
        let local = HSTRING::from(path.as_os_str());
        let source = HSTRING::from(source);
        let guid = GUID::from_u128(DOWNOVA_CLIENT_GUID);
        // The object lives in this block and is released before `_apartment` uninitializes COM.
        let result = (|| -> windows::core::Result<()> {
            // SAFETY: a registered in-process class with its documented interface; the returned
            // interface pointer is owned by `exec` and released by its Drop on this thread.
            let exec: IAttachmentExecute =
                unsafe { CoCreateInstance(&AttachmentServices, None, CLSCTX_INPROC_SERVER)? };
            // SAFETY: `guid`, `local` and `source` outlive the calls; the strings are
            // NUL-terminated wide strings owned by HSTRING.
            unsafe {
                exec.SetClientGuid(&guid)?;
                exec.SetLocalPath(&local)?;
                exec.SetSource(&source)?;
                exec.Save()
            }
        })();
        match result {
            Ok(()) => SaveResult::Saved,
            Err(e) if e.code().0 == INET_E_SECURITY_PROBLEM => SaveResult::PolicyBlocked,
            Err(e) => SaveResult::Failed(e.code().0),
        }
    }

    #[allow(unsafe_code)]
    pub(super) fn named_streams_supported(path: &Path) -> Option<bool> {
        let wide = HSTRING::from(path.as_os_str());
        let mut root = [0u16; 1024];
        // SAFETY: `wide` is a valid NUL-terminated path; `root` is a writable buffer whose length
        // the binding passes.
        unsafe { GetVolumePathNameW(&wide, &mut root) }.ok()?;
        let mut flags = 0u32;
        // SAFETY: `root` holds the NUL-terminated volume root just written; only the flags output
        // is requested, through a valid pointer.
        unsafe {
            GetVolumeInformationW(
                windows::core::PCWSTR(root.as_ptr()),
                None,
                None,
                None,
                Some(&mut flags),
                None,
            )
        }
        .ok()?;
        Some(flags & FILE_NAMED_STREAMS != 0)
    }

    #[allow(unsafe_code)]
    pub(super) fn file_identity(path: &Path) -> std::io::Result<Option<String>> {
        let file = std::fs::File::open(path)?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the handle belongs to `file`, which stays open for the call; `info` is a valid
        // output buffer.
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
        Ok(Some(format!(
            "{:08x}:{index:016x}",
            info.dwVolumeSerialNumber
        )))
    }
}

#[cfg(not(windows))]
mod imp {
    use super::SaveResult;
    use std::path::Path;

    pub(super) fn save(_path: &Path, _source: &str) -> SaveResult {
        SaveResult::NotSupported
    }
    pub(super) fn named_streams_supported(_path: &Path) -> Option<bool> {
        None
    }
    pub(super) fn file_identity(_path: &Path) -> std::io::Result<Option<String>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_policy_block_code_is_inet_e_security_problem() {
        assert_eq!(INET_E_SECURITY_PROBLEM as u32, 0x800C_000E);
    }

    #[cfg(windows)]
    #[test]
    fn identity_follows_the_file_not_the_name() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.bin");
        let b = dir.path().join("b.bin");
        std::fs::write(&a, b"one").unwrap();
        std::fs::hard_link(&a, &b).unwrap();
        let ia = file_identity(&a).unwrap().unwrap();
        assert_eq!(
            file_identity(&b).unwrap().unwrap(),
            ia,
            "a hard link is the same file"
        );
        std::fs::remove_file(&b).unwrap();
        std::fs::write(&b, b"one").unwrap();
        assert_ne!(
            file_identity(&b).unwrap().unwrap(),
            ia,
            "same bytes, another file"
        );
        assert!(named_streams_supported(dir.path()).is_some());
    }

    /// Real Windows Attachment Services on a temporary file (ignored by default: it touches the
    /// user's security configuration only through the documented API, but its outcome depends
    /// on the machine's policy and antivirus). `cargo test -p dm-win-attachment -- --ignored`.
    #[cfg(windows)]
    #[test]
    #[ignore = "real Attachment Services; run explicitly"]
    fn real_attachment_services_save() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("downova-attachment-check.txt");
        std::fs::write(&file, b"plain text, harmless").unwrap();
        let path = file.clone();
        let result = std::thread::spawn(move || save(&path, "https://downloads.example.org/"))
            .join()
            .unwrap();
        println!("Save → {result:?}");
        let zone = std::fs::read_to_string(format!("{}:Zone.Identifier", file.display()));
        println!("Zone.Identifier → {zone:?}");
        assert_ne!(result, SaveResult::NotSupported);
        assert!(file.exists(), "a harmless text file is kept");
    }
}
