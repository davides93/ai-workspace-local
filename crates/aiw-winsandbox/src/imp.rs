use crate::{Access, Outcome};
use std::collections::HashSet;
use std::ffi::{c_void, OsStr};
use std::fs::File;
use std::io::{self, Read};
use std::mem::{size_of, size_of_val, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, SetHandleInformation, ERROR_SUCCESS, GENERIC_ALL, HANDLE, HANDLE_FLAG_INHERIT,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, ConvertStringSidToSidW, GetNamedSecurityInfoW,
    SetNamedSecurityInfoW, SetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT, SE_WINDOW_OBJECT,
};
use windows_sys::Win32::Security::{
    CreateRestrictedToken, EqualSid, GetAce, GetLengthSid, GetSecurityDescriptorDacl, GetSecurityDescriptorSacl, GetTokenInformation,
    SetTokenInformation, TokenIntegrityLevel, TokenUser, ACE_HEADER, ACL, CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION,
    DISABLE_MAX_PRIVILEGE, LABEL_SECURITY_INFORMATION, OBJECT_INHERIT_ACE, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
    SID_AND_ATTRIBUTES, SYSTEM_MANDATORY_LABEL_ACE, TOKEN_ADJUST_DEFAULT, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_MANDATORY_LABEL,
    TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicUIRestrictions, JobObjectExtendedLimitInformation, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_UI_RESTRICTIONS, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_UILIMIT_DESKTOP, JOB_OBJECT_UILIMIT_DISPLAYSETTINGS, JOB_OBJECT_UILIMIT_EXITWINDOWS,
    JOB_OBJECT_UILIMIT_GLOBALATOMS, JOB_OBJECT_UILIMIT_HANDLES, JOB_OBJECT_UILIMIT_READCLIPBOARD, JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS,
    JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::StationsAndDesktops::{
    CreateDesktopW, CreateWindowStationW, GetProcessWindowStation, GetUserObjectInformationW, SetProcessWindowStation, UOI_NAME,
};
use windows_sys::Win32::System::SystemServices::{SE_GROUP_INTEGRITY, SYSTEM_MANDATORY_LABEL_ACE_TYPE};
use windows_sys::Win32::System::Threading::{
    CreateProcessAsUserW, DeleteProcThreadAttributeList, GetCurrentProcess, GetExitCodeProcess, InitializeProcThreadAttributeList,
    OpenProcessToken, ResumeThread, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED,
    CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

/// Low mandatory level and the label giving it to a folder and everything below it.
const LOW_SID: &str = "S-1-16-4096";
const LOW_LABEL_SDDL: &str = "S:(ML;OICI;NW;;;LW)";
/// BUILTIN\Administrators: deny-only in the sandbox token, in case the app runs elevated.
const ADMINS_SID: &str = "S-1-5-32-544";

/// Closes a kernel handle on drop.
struct Owned(HANDLE);
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// A SID allocated by `ConvertStringSidToSidW`.
struct LocalSid(PSID);
impl Drop for LocalSid {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0 as _) };
    }
}

struct AttrList(LPPROC_THREAD_ATTRIBUTE_LIST);
impl Drop for AttrList {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.0) };
    }
}

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

fn os_error(what: &str) -> io::Error {
    let e = io::Error::last_os_error();
    io::Error::new(e.kind(), format!("{what}: {e}"))
}

fn check(ok: i32, what: &str) -> io::Result<()> {
    if ok == 0 {
        Err(os_error(what))
    } else {
        Ok(())
    }
}

fn sid(s: &str) -> io::Result<LocalSid> {
    let mut p: PSID = null_mut();
    check(unsafe { ConvertStringSidToSidW(wide(s).as_ptr(), &mut p) }, "ConvertStringSidToSid")?;
    Ok(LocalSid(p))
}

/// Privilege-less copy of this process token at Low integrity (optionally Administrators deny-only).
fn low_token(h: Hardening) -> io::Result<Owned> {
    unsafe {
        let mut process_token: HANDLE = null_mut();
        check(
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT,
                &mut process_token,
            ),
            "OpenProcessToken",
        )?;
        let process_token = Owned(process_token);
        let admins = sid(ADMINS_SID)?;
        let disable = [SID_AND_ATTRIBUTES { Sid: admins.0, Attributes: 0 }];
        let n_disable = u32::from(h.deny_admins);
        let mut restricted: HANDLE = null_mut();
        check(
            CreateRestrictedToken(
                process_token.0,
                DISABLE_MAX_PRIVILEGE,
                n_disable,
                disable.as_ptr(),
                0,
                null(),
                0,
                null(),
                &mut restricted,
            ),
            "CreateRestrictedToken",
        )?;
        let restricted = Owned(restricted);
        if !h.low_integrity {
            return Ok(restricted);
        }
        let low = sid(LOW_SID)?;
        let label = TOKEN_MANDATORY_LABEL { Label: SID_AND_ATTRIBUTES { Sid: low.0, Attributes: SE_GROUP_INTEGRITY as u32 } };
        let size = size_of::<TOKEN_MANDATORY_LABEL>() as u32 + GetLengthSid(low.0);
        check(SetTokenInformation(restricted.0, TokenIntegrityLevel, &label as *const _ as *const c_void, size), "SetTokenInformation")?;
        Ok(restricted)
    }
}

/// Whether `dir` carries an inheritable Low integrity label.
pub fn has_low_label(dir: &Path) -> bool {
    unsafe {
        let path = wide(dir);
        let mut sacl: *mut ACL = null_mut();
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        let err = GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            LABEL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut sacl,
            &mut sd,
        );
        if err != ERROR_SUCCESS {
            return false;
        }
        let mut found = false;
        if let (false, Ok(low)) = (sacl.is_null(), sid(LOW_SID)) {
            let inherit = (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE) as u8;
            for i in 0..(*sacl).AceCount as u32 {
                let mut ace: *mut c_void = null_mut();
                if GetAce(sacl, i, &mut ace) == 0 {
                    continue;
                }
                let header = &*(ace as *const ACE_HEADER);
                if header.AceType as u32 == SYSTEM_MANDATORY_LABEL_ACE_TYPE && header.AceFlags & inherit == inherit {
                    let label = ace as *const SYSTEM_MANDATORY_LABEL_ACE;
                    let label_sid = std::ptr::addr_of!((*label).SidStart) as PSID;
                    if EqualSid(label_sid, low.0) != 0 {
                        found = true;
                    }
                }
            }
        }
        LocalFree(sd as _);
        found
    }
}

/// Gives `dir` (and, by inheritance, its content) the Low integrity label, so that sandboxed
/// commands can write there. Changes the folder's security descriptor persistently.
pub fn label_low(dir: &Path) -> io::Result<()> {
    unsafe {
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        check(
            ConvertStringSecurityDescriptorToSecurityDescriptorW(wide(LOW_LABEL_SDDL).as_ptr(), SDDL_REVISION_1, &mut sd, null_mut()),
            "ConvertStringSecurityDescriptor",
        )?;
        let (mut present, mut defaulted) = (0, 0);
        let mut sacl: *mut ACL = null_mut();
        let err = if GetSecurityDescriptorSacl(sd, &mut present, &mut sacl, &mut defaulted) == 0 {
            GetLastError()
        } else {
            SetNamedSecurityInfoW(wide(dir).as_ptr(), SE_FILE_OBJECT, LABEL_SECURITY_INFORMATION, null_mut(), null_mut(), null(), sacl)
        };
        LocalFree(sd as _);
        if err != ERROR_SUCCESS {
            return Err(io::Error::new(
                io::Error::from_raw_os_error(err as i32).kind(),
                format!("etichetta Low su {}: errore {err}", dir.display()),
            ));
        }
        Ok(())
    }
}

/// SID of the user running the app, as a string (for SDDL).
fn current_user_sid() -> std::result::Result<String, String> {
    unsafe {
        let mut token: HANDLE = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(format!("OpenProcessToken: {}", io::Error::last_os_error()));
        }
        let token = Owned(token);
        let mut len = 0u32;
        GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut len);
        let mut buf = vec![0u8; len.max(64) as usize];
        if GetTokenInformation(token.0, TokenUser, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut len) == 0 {
            return Err(format!("GetTokenInformation: {}", io::Error::last_os_error()));
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut text: *mut u16 = null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
            return Err(format!("ConvertSidToStringSid: {}", io::Error::last_os_error()));
        }
        let mut n = 0;
        while *text.add(n) != 0 {
            n += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, n));
        LocalFree(text as _);
        Ok(sid)
    }
}

/// Security of the sandbox window station and desktop: full access for the user (whose token,
/// even restricted, keeps the user SID) and SYSTEM, Low integrity label. An explicit DACL is
/// needed because an elevated creator's default DACL only names Administrators, which are
/// deny-only in the sandbox token.
fn secure_window_object(handle: HANDLE, user_sid: &str, what: &str) -> std::result::Result<(), String> {
    unsafe {
        let sddl = format!("D:(A;;GA;;;{user_sid})(A;;GA;;;SY)S:(ML;;NW;;;LW)");
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(wide(&sddl).as_ptr(), SDDL_REVISION_1, &mut sd, null_mut()) == 0 {
            return Err(format!("sicurezza {what}: {}", io::Error::last_os_error()));
        }
        let (mut present, mut defaulted) = (0, 0);
        let mut dacl: *mut ACL = null_mut();
        let mut sacl: *mut ACL = null_mut();
        let err = if GetSecurityDescriptorDacl(sd, &mut present, &mut dacl, &mut defaulted) == 0
            || GetSecurityDescriptorSacl(sd, &mut present, &mut sacl, &mut defaulted) == 0
        {
            GetLastError()
        } else {
            SetSecurityInfo(
                handle,
                SE_WINDOW_OBJECT,
                DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                dacl,
                sacl,
            )
        };
        LocalFree(sd as _);
        if err != ERROR_SUCCESS {
            return Err(format!("sicurezza {what}: errore {err}"));
        }
        Ok(())
    }
}

fn object_name(handle: HANDLE) -> std::result::Result<String, String> {
    unsafe {
        let mut needed = 0u32;
        GetUserObjectInformationW(handle, UOI_NAME, null_mut(), 0, &mut needed);
        let mut buf = vec![0u16; (needed as usize / 2).max(64)];
        if GetUserObjectInformationW(handle, UOI_NAME, buf.as_mut_ptr() as *mut c_void, (buf.len() * 2) as u32, &mut needed) == 0 {
            return Err(format!("nome della window station: {}", io::Error::last_os_error()));
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Ok(String::from_utf16_lossy(&buf[..end]))
    }
}

/// Window station and desktop reserved to sandboxed commands (the approach of Chromium's
/// sandbox): labelled Low, so that Low integrity processes can initialise user32 even when the
/// app runs in a service session, and isolated from the user's clipboard, atoms and windows.
/// Created once; the handles stay open for the life of the process.
fn sandbox_desktop() -> io::Result<&'static [u16]> {
    static DESKTOP: OnceLock<std::result::Result<Vec<u16>, String>> = OnceLock::new();
    let created = DESKTOP.get_or_init(|| unsafe {
        let winsta = CreateWindowStationW(null(), 0, GENERIC_ALL, null());
        if winsta.is_null() {
            return Err(format!("CreateWindowStation: {}", io::Error::last_os_error()));
        }
        let name = object_name(winsta)?;
        // A desktop is created in the process window station: switch to the new one briefly.
        let previous = GetProcessWindowStation();
        if SetProcessWindowStation(winsta) == 0 {
            return Err(format!("SetProcessWindowStation: {}", io::Error::last_os_error()));
        }
        let desktop = CreateDesktopW(wide("aiw-sandbox").as_ptr(), null(), null(), 0, GENERIC_ALL, null());
        let desktop_error = io::Error::last_os_error();
        SetProcessWindowStation(previous);
        if desktop.is_null() {
            return Err(format!("CreateDesktop: {desktop_error}"));
        }
        let user = current_user_sid()?;
        secure_window_object(winsta, &user, "window station")?;
        secure_window_object(desktop, &user, "desktop")?;
        Ok(wide(format!("{name}\\aiw-sandbox")))
    });
    created.as_deref().map_err(|e| io::Error::other(e.clone()))
}

/// Labels the workspace once per process (labelling walks the whole tree).
fn ensure_low_label(dir: &Path) -> io::Result<()> {
    static DONE: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    let done = DONE.get_or_init(Default::default);
    if done.lock().unwrap().contains(dir) {
        return Ok(());
    }
    if !has_low_label(dir) {
        label_low(dir)?;
    }
    done.lock().unwrap().insert(dir.to_path_buf());
    Ok(())
}

/// TEMP for sandboxed commands: under the user's LocalLow folder (Low integrity by default).
fn low_temp_dir(workspace: &Path) -> io::Result<PathBuf> {
    let dir = match std::env::var_os("USERPROFILE") {
        Some(home) => PathBuf::from(home).join("AppData").join("LocalLow").join("AI Workspace").join("tmp"),
        None => workspace.join(".aiw").join("tmp"),
    };
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn new_job() -> io::Result<Owned> {
    unsafe {
        let job = CreateJobObjectW(null(), null());
        if job.is_null() {
            return Err(os_error("CreateJobObject"));
        }
        let job = Owned(job);
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
        check(
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const c_void,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ),
            "SetInformationJobObject(limiti)",
        )?;
        let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
            UIRestrictionsClass: JOB_OBJECT_UILIMIT_DESKTOP
                | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
                | JOB_OBJECT_UILIMIT_EXITWINDOWS
                | JOB_OBJECT_UILIMIT_GLOBALATOMS
                | JOB_OBJECT_UILIMIT_HANDLES
                | JOB_OBJECT_UILIMIT_READCLIPBOARD
                | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS
                | JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
        };
        check(
            SetInformationJobObject(
                job.0,
                JobObjectBasicUIRestrictions,
                &ui as *const _ as *const c_void,
                size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>() as u32,
            ),
            "SetInformationJobObject(UI)",
        )?;
        Ok(job)
    }
}

/// Environment of this process with `overrides` applied, as a sorted UTF-16 block.
fn env_block(overrides: &[(String, String)]) -> Vec<u16> {
    let mut vars: Vec<(String, String)> =
        std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?))).collect();
    for (k, v) in overrides {
        vars.retain(|(x, _)| !x.eq_ignore_ascii_case(k));
        vars.push((k.clone(), v.clone()));
    }
    vars.sort_by_key(|(k, _)| k.to_uppercase());
    let mut block = Vec::new();
    for (k, v) in vars {
        block.extend(OsStr::new(&format!("{k}={v}")).encode_wide());
        block.push(0);
    }
    block.push(0);
    block
}

/// Runs `command_line` (passed verbatim to `CreateProcessAsUserW`) in the sandbox.
///
/// `cancel` is polled every 100 ms; on timeout or cancellation the whole process tree is killed.
/// Processes the command leaves running in the background are killed when it exits.
#[allow(clippy::too_many_arguments)]
pub fn run(
    command_line: &str,
    cwd: &Path,
    workspace: &Path,
    access: Access,
    extra_env: &[(String, String)],
    timeout: Duration,
    cancel: &AtomicBool,
    max_output: usize,
) -> io::Result<Outcome> {
    run_with(command_line, cwd, workspace, access, extra_env, timeout, cancel, max_output, Hardening::default())
}

/// The individual protections, all on by default (switchable only for diagnostics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hardening {
    pub low_integrity: bool,
    pub deny_admins: bool,
    pub own_desktop: bool,
}

impl Default for Hardening {
    fn default() -> Self {
        // deny_admins stays off: on an elevated token (e.g. a CI runner or a service) it stops
        // processes from initialising (0xC0000142), and UAC-filtered tokens already carry
        // Administrators as deny-only. Low integrity blocks writes before the DACL is checked.
        Self { low_integrity: true, deny_admins: false, own_desktop: true }
    }
}

#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn run_with(
    command_line: &str,
    cwd: &Path,
    workspace: &Path,
    access: Access,
    extra_env: &[(String, String)],
    timeout: Duration,
    cancel: &AtomicBool,
    max_output: usize,
    hardening: Hardening,
) -> io::Result<Outcome> {
    if access == Access::WorkspaceWrite {
        ensure_low_label(workspace)?;
    }
    let temp = low_temp_dir(workspace)?.display().to_string();
    let mut env = vec![
        ("TEMP".to_string(), temp.clone()),
        ("TMP".to_string(), temp.clone()),
        ("PSModuleAnalysisCachePath".to_string(), format!("{temp}\\ps-module-cache")),
    ];
    env.extend_from_slice(extra_env);
    let token = low_token(hardening)?;
    let job = new_job()?;
    let desktop = if hardening.own_desktop { Some(sandbox_desktop()?) } else { None };

    unsafe {
        // One pipe for stdout and stderr, so the output keeps its order.
        let sa =
            SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: null_mut(), bInheritHandle: 1 };
        let (mut r, mut w): (HANDLE, HANDLE) = (null_mut(), null_mut());
        check(CreatePipe(&mut r, &mut w, &sa, 0), "CreatePipe")?;
        let read = Owned(r);
        let write = Owned(w);
        check(SetHandleInformation(read.0, HANDLE_FLAG_INHERIT, 0), "SetHandleInformation")?;
        let nul = CreateFileW(
            wide("NUL").as_ptr(),
            FILE_GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &sa,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        );
        if nul == INVALID_HANDLE_VALUE {
            return Err(os_error("CreateFile(NUL)"));
        }
        let nul = Owned(nul);

        // The child inherits exactly these handles, whatever else is inheritable in this process.
        let handles = [nul.0, write.0];
        let mut size = 0usize;
        InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size);
        let mut attr_buf = vec![0u8; size];
        let attrs = attr_buf.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
        check(InitializeProcThreadAttributeList(attrs, 1, 0, &mut size), "InitializeProcThreadAttributeList")?;
        let attr_guard = AttrList(attrs);
        check(
            UpdateProcThreadAttribute(
                attrs,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr() as *const c_void,
                size_of_val(&handles),
                null_mut(),
                null(),
            ),
            "UpdateProcThreadAttribute",
        )?;

        let mut si: STARTUPINFOEXW = zeroed();
        si.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        if let Some(d) = desktop {
            si.StartupInfo.lpDesktop = d.as_ptr() as *mut u16;
        }
        si.StartupInfo.hStdInput = nul.0;
        si.StartupInfo.hStdOutput = write.0;
        si.StartupInfo.hStdError = write.0;
        si.lpAttributeList = attrs;

        let mut cmd = wide(command_line);
        let block = env_block(&env);
        let cwd_w = wide(cwd);
        let mut pi: PROCESS_INFORMATION = zeroed();
        check(
            CreateProcessAsUserW(
                token.0,
                null(),
                cmd.as_mut_ptr(),
                null(),
                null(),
                1,
                CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
                block.as_ptr() as *const c_void,
                cwd_w.as_ptr(),
                &si.StartupInfo,
                &mut pi,
            ),
            "CreateProcessAsUser",
        )?;
        drop(attr_guard);
        drop(attr_buf);
        let process = Owned(pi.hProcess);
        let thread = Owned(pi.hThread);
        if AssignProcessToJobObject(job.0, process.0) == 0 {
            let e = os_error("AssignProcessToJobObject");
            TerminateProcess(process.0, 1);
            return Err(e);
        }
        ResumeThread(thread.0);
        // The child has its own copies: keeping ours open would stop the pipe from ever closing.
        drop(write);
        drop(nul);

        let mut file = File::from_raw_handle(read.0 as RawHandle);
        std::mem::forget(read);
        let reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                match file.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let room = max_output.saturating_sub(buf.len());
                        buf.extend_from_slice(&chunk[..n.min(room)]);
                    }
                }
            }
            buf
        });

        let start = Instant::now();
        let (mut timed_out, mut cancelled) = (false, false);
        loop {
            match WaitForSingleObject(process.0, 100) {
                WAIT_OBJECT_0 => break,
                WAIT_TIMEOUT => {
                    if cancel.load(Ordering::Relaxed) {
                        cancelled = true;
                    } else if start.elapsed() >= timeout {
                        timed_out = true;
                    }
                    if timed_out || cancelled {
                        TerminateJobObject(job.0, 1);
                        WaitForSingleObject(process.0, 5_000);
                        break;
                    }
                }
                _ => {
                    TerminateJobObject(job.0, 1);
                    return Err(os_error("WaitForSingleObject"));
                }
            }
        }
        let mut code = 0u32;
        let exit_code = if timed_out || cancelled || GetExitCodeProcess(process.0, &mut code) == 0 { None } else { Some(code) };
        // Background processes started by the command end with it (like bwrap --die-with-parent).
        TerminateJobObject(job.0, 1);
        let output = reader.join().unwrap_or_default();
        Ok(Outcome { exit_code, output, timed_out, cancelled })
    }
}
