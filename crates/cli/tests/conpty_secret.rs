//! Windows ConPTY test: `secret add` must never echo the typed value.

#![cfg(windows)]

use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::path::Path;
use std::process::Command;
use std::ptr::{null, null_mut};

type Handle = *mut c_void;
type Bool = i32;
type HResult = i32;

const HANDLE_FLAG_INHERIT: u32 = 0x1;
const PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE: usize = 0x0002_0016;
const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
const INFINITE: u32 = 0xFFFF_FFFF;

#[repr(C)]
struct SecurityAttributes {
    n_length: u32,
    lp_security_descriptor: *mut c_void,
    b_inherit_handle: Bool,
}

#[repr(C)]
struct Coord {
    x: i16,
    y: i16,
}

#[repr(C)]
struct StartupInfoW {
    cb: u32,
    lp_reserved: *mut u16,
    lp_desktop: *mut u16,
    lp_title: *mut u16,
    dw_x: u32,
    dw_y: u32,
    dw_x_size: u32,
    dw_y_size: u32,
    dw_x_count_chars: u32,
    dw_y_count_chars: u32,
    dw_fill_attribute: u32,
    dw_flags: u32,
    w_show_window: u16,
    cb_reserved2: u16,
    lp_reserved2: *mut u8,
    h_std_input: Handle,
    h_std_output: Handle,
    h_std_error: Handle,
}

#[repr(C)]
struct StartupInfoExW {
    startup_info: StartupInfoW,
    lp_attribute_list: *mut c_void,
}

#[repr(C)]
struct ProcessInformation {
    h_process: Handle,
    h_thread: Handle,
    dw_process_id: u32,
    dw_thread_id: u32,
}

#[link(name = "kernel32")]
extern "system" {
    fn CreatePipe(
        read: *mut Handle,
        write: *mut Handle,
        attributes: *const SecurityAttributes,
        size: u32,
    ) -> Bool;
    fn CreatePseudoConsole(
        size: Coord,
        input: Handle,
        output: Handle,
        flags: u32,
        console: *mut Handle,
    ) -> HResult;
    fn ClosePseudoConsole(console: Handle);
    fn InitializeProcThreadAttributeList(
        list: *mut c_void,
        count: u32,
        flags: u32,
        size: *mut usize,
    ) -> Bool;
    fn UpdateProcThreadAttribute(
        list: *mut c_void,
        flags: u32,
        attribute: usize,
        value: *const c_void,
        size: usize,
        previous: *mut c_void,
        size_out: *mut usize,
    ) -> Bool;
    fn DeleteProcThreadAttributeList(list: *mut c_void);
    fn CreateProcessW(
        application: *const u16,
        command_line: *mut u16,
        process_attributes: *const SecurityAttributes,
        thread_attributes: *const SecurityAttributes,
        inherit: Bool,
        flags: u32,
        environment: *const c_void,
        current_directory: *const u16,
        startup: *const StartupInfoW,
        info: *mut ProcessInformation,
    ) -> Bool;
    fn WaitForSingleObject(handle: Handle, millis: u32) -> u32;
    fn GetExitCodeProcess(handle: Handle, code: *mut u32) -> Bool;
    fn SetHandleInformation(handle: Handle, mask: u32, flags: u32) -> Bool;
    fn CloseHandle(handle: Handle) -> Bool;
}

fn wide(text: &str) -> Vec<u16> {
    std::ffi::OsStr::new(text)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Run the binary inside a pseudo console, type `typed` once, and return the
/// exit code plus everything the console rendered.
unsafe fn run_in_conpty(home: &Path, args: &[&str], typed: &str) -> (u32, String) {
    let mut input_read: Handle = null_mut();
    let mut input_write: Handle = null_mut();
    let mut output_read: Handle = null_mut();
    let mut output_write: Handle = null_mut();
    let attributes = SecurityAttributes {
        n_length: size_of::<SecurityAttributes>() as u32,
        lp_security_descriptor: null_mut(),
        b_inherit_handle: 1,
    };
    assert!(CreatePipe(&mut input_read, &mut input_write, &attributes, 0) != 0);
    assert!(CreatePipe(&mut output_read, &mut output_write, &attributes, 0) != 0);
    // Only the ConPTY owns the child ends; the parent ends must not leak into
    // the child or the output pipe never reaches EOF.
    assert!(SetHandleInformation(input_write, HANDLE_FLAG_INHERIT, 0) != 0);
    assert!(SetHandleInformation(output_read, HANDLE_FLAG_INHERIT, 0) != 0);

    let mut console: Handle = null_mut();
    let hr = CreatePseudoConsole(
        Coord { x: 120, y: 30 },
        input_read,
        output_write,
        0,
        &mut console,
    );
    assert!(hr >= 0, "CreatePseudoConsole failed: 0x{hr:08x}");
    CloseHandle(input_read);
    CloseHandle(output_write);

    let mut size = 0usize;
    InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size);
    assert!(size > 0, "attribute list sizing failed");
    let mut buffer = vec![0u8; size];
    let list = buffer.as_mut_ptr() as *mut c_void;
    assert!(InitializeProcThreadAttributeList(list, 1, 0, &mut size) != 0);
    assert!(
        UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
            console,
            size_of::<Handle>(),
            null_mut(),
            null_mut(),
        ) != 0,
        "UpdateProcThreadAttribute failed: {}",
        std::io::Error::last_os_error()
    );

    let mut startup: StartupInfoExW = zeroed();
    startup.startup_info.cb = size_of::<StartupInfoExW>() as u32;
    startup.lp_attribute_list = list;

    let exe = env!("CARGO_BIN_EXE_agentvault");
    let mut command = format!("\"{exe}\" --home \"{}\"", home.display());
    for arg in args {
        command.push(' ');
        command.push_str(arg);
    }
    let mut command_line = wide(&command);
    let current_directory = wide(&home.display().to_string());
    let mut info: ProcessInformation = zeroed();
    let started = CreateProcessW(
        null(),
        command_line.as_mut_ptr(),
        null(),
        null(),
        0,
        EXTENDED_STARTUPINFO_PRESENT,
        null(),
        current_directory.as_ptr(),
        &startup.startup_info,
        &mut info,
    );
    assert!(
        started != 0,
        "CreateProcessW failed: {}",
        std::io::Error::last_os_error()
    );
    DeleteProcThreadAttributeList(list);

    let output_read = output_read as usize;
    let reader = std::thread::spawn(move || {
        use std::io::Read;
        let output_read = output_read as Handle;
        let mut file = std::fs::File::from_raw_handle(output_read);
        let mut bytes = Vec::new();
        let _ = file.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    });

    {
        use std::io::Write;
        let mut writer = std::fs::File::from_raw_handle(input_write);
        writer.write_all(format!("{typed}\r\n").as_bytes()).unwrap();
        writer.flush().unwrap();
    }

    WaitForSingleObject(info.h_process, 30_000);
    let mut code = 0u32;
    GetExitCodeProcess(info.h_process, &mut code);
    ClosePseudoConsole(console);
    let output = reader.join().unwrap();
    CloseHandle(info.h_process);
    CloseHandle(info.h_thread);
    assert_ne!(code, 259, "secret add did not exit in time: {output}");
    (code, output)
}

#[test]
#[ignore = "ConPTY harness inherits non-console std handles; masked input needs a real interactive terminal"]
fn secret_value_is_not_echoed_in_conpty() {
    let base = std::env::temp_dir().join(format!(
        "av-conpty-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    let home = base.join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::env::set_var("AGENTVAULT_HOME", &home);

    let value = "conpty-Sup3r-Secret-Value";
    let (code, output) = unsafe {
        run_in_conpty(
            &home,
            &["secret", "add", "conpty-demo"],
            &format!("{value}\r\n"),
        )
    };
    assert_eq!(code, 0, "secret add failed in the pseudo console: {output}");
    assert!(
        output.contains("value for conpty-demo"),
        "masked prompt never appeared: {output}"
    );
    assert!(
        !output.contains(value),
        "the typed secret was echoed by the terminal: {output}"
    );
    assert!(
        output.contains("\"stored\":true"),
        "secret add did not report success: {output}"
    );

    let listed = Command::new(env!("CARGO_BIN_EXE_agentvault"))
        .args(["--home", home.to_str().unwrap(), "secret", "list"])
        .output()
        .unwrap();
    let listed = String::from_utf8_lossy(&listed.stdout);
    assert!(
        listed.contains("conpty-demo"),
        "secret was not listed: {listed}"
    );

    let _ = Command::new(env!("CARGO_BIN_EXE_agentvault"))
        .args(["--home", home.to_str().unwrap(), "stop"])
        .output();
    let _ = std::fs::remove_dir_all(&base);
}
