//! Windows service control through the Service Control Manager.

use std::mem::size_of;
use std::time::{Duration, Instant};

use tweaker_core::action::ServiceStartMode;
use tweaker_core::adapters::{AdapterError, IServiceControl, ServiceState, ServiceStatus};
use windows::Win32::System::Services::{
    ChangeServiceConfigW, CloseServiceHandle, ControlService, ENUM_SERVICE_TYPE, OpenSCManagerW,
    OpenServiceW, QUERY_SERVICE_CONFIGW, QueryServiceConfigW, QueryServiceStatusEx, SC_HANDLE,
    SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO, SERVICE_CHANGE_CONFIG, SERVICE_CONTINUE_PENDING,
    SERVICE_CONTROL_STOP, SERVICE_ERROR, SERVICE_NO_CHANGE, SERVICE_PAUSE_PENDING, SERVICE_PAUSED,
    SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START,
    SERVICE_START_PENDING, SERVICE_STATUS, SERVICE_STATUS_CURRENT_STATE, SERVICE_STATUS_PROCESS,
    SERVICE_STOP, SERVICE_STOP_PENDING, SERVICE_STOPPED, StartServiceW,
};
use windows::core::PCWSTR;

/// Service Control Manager access.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsServices;

/// How long a start/stop may take before it counts as failed.
const SERVICE_TIMEOUT: Duration = Duration::from_secs(20);

struct ScHandle(SC_HANDLE);

impl Drop for ScHandle {
    fn drop(&mut self) {
        // SAFETY: the handle was returned by the SCM and is closed exactly once.
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

impl IServiceControl for WindowsServices {
    fn query(&self, service: &str) -> Result<Option<ServiceState>, AdapterError> {
        let scm = open_scm()?;
        let handle = match open_service(&scm, service, SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG)
        {
            Ok(handle) => handle,
            Err(error) if is_missing(&error) => return Ok(None),
            Err(error) => return Err(service_error(service, "open", error)),
        };
        let status = query_status(&handle, service)?;
        let start_mode = query_start_mode(&handle, service)?;
        Ok(Some(ServiceState { status, start_mode }))
    }

    fn start(&self, service: &str) -> Result<(), AdapterError> {
        let scm = open_scm()?;
        let handle = open_service(&scm, service, SERVICE_START | SERVICE_QUERY_STATUS)
            .map_err(|error| service_error(service, "open for start", error))?;
        // SAFETY: the handle is valid and no service arguments are needed.
        unsafe { StartServiceW(handle.0, None) }
            .map_err(|error| service_error(service, "start", error))?;
        wait_for(&handle, service, ServiceStatus::Running, SERVICE_TIMEOUT)
    }

    fn stop(&self, service: &str) -> Result<(), AdapterError> {
        let scm = open_scm()?;
        let handle = open_service(&scm, service, SERVICE_STOP | SERVICE_QUERY_STATUS)
            .map_err(|error| service_error(service, "open for stop", error))?;
        let mut status = SERVICE_STATUS::default();
        // SAFETY: the handle is valid and `status` is a writable out-parameter.
        unsafe { ControlService(handle.0, SERVICE_CONTROL_STOP, &mut status) }
            .map_err(|error| service_error(service, "stop", error))?;
        wait_for(&handle, service, ServiceStatus::Stopped, SERVICE_TIMEOUT)
    }

    fn set_start_mode(&self, service: &str, mode: ServiceStartMode) -> Result<(), AdapterError> {
        let scm = open_scm()?;
        let handle = open_service(&scm, service, SERVICE_CHANGE_CONFIG)
            .map_err(|error| service_error(service, "open for configure", error))?;
        let no_change = PCWSTR::null();
        // SAFETY: the handle is valid; NULL means "leave this field unchanged".
        unsafe {
            ChangeServiceConfigW(
                handle.0,
                ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE),
                windows::Win32::System::Services::SERVICE_START_TYPE(mode.to_windows()),
                SERVICE_ERROR(SERVICE_NO_CHANGE),
                no_change,
                no_change,
                None,
                no_change,
                no_change,
                no_change,
                no_change,
            )
        }
        .map_err(|error| service_error(service, "change start mode", error))
    }
}

fn open_scm() -> Result<ScHandle, AdapterError> {
    // SAFETY: opening the local service manager with a minimal access mask.
    unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) }
        .map(ScHandle)
        .map_err(|error| AdapterError::new(format!("cannot open the service manager: {error}")))
}

fn open_service(
    scm: &ScHandle,
    service: &str,
    access: u32,
) -> Result<ScHandle, windows::core::Error> {
    let name = wide(service);
    // SAFETY: `name` is NUL-terminated and lives for the call.
    unsafe { OpenServiceW(scm.0, PCWSTR(name.as_ptr()), access) }.map(ScHandle)
}

fn query_status(handle: &ScHandle, service: &str) -> Result<ServiceStatus, AdapterError> {
    let mut buffer = [0u8; size_of::<SERVICE_STATUS_PROCESS>()];
    let mut needed = 0u32;
    // SAFETY: the buffer is exactly the size of the structure the API writes.
    unsafe {
        QueryServiceStatusEx(
            handle.0,
            SC_STATUS_PROCESS_INFO,
            Some(&mut buffer),
            &mut needed,
        )
    }
    .map_err(|error| service_error(service, "query status", error))?;
    // SAFETY: `buffer` was filled by the API and has the right size and alignment.
    let status: SERVICE_STATUS_PROCESS =
        unsafe { std::ptr::read_unaligned(buffer.as_ptr() as *const SERVICE_STATUS_PROCESS) };
    Ok(map_status(status.dwCurrentState))
}

fn query_start_mode(handle: &ScHandle, service: &str) -> Result<ServiceStartMode, AdapterError> {
    let mut needed = 0u32;
    // First call only asks for the required buffer size; the failure is expected.
    // SAFETY: null buffer, zero size — documented way to query the size.
    let _ = unsafe { QueryServiceConfigW(handle.0, None, 0, &mut needed) };
    if needed == 0 {
        return Err(AdapterError::new(format!(
            "service {service}: cannot determine the configuration size"
        )));
    }
    let mut buffer = vec![0u8; needed as usize];
    // SAFETY: the buffer is large enough and the API writes at most `needed` bytes.
    unsafe {
        QueryServiceConfigW(
            handle.0,
            Some(buffer.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW),
            needed,
            &mut needed,
        )
    }
    .map_err(|error| service_error(service, "query configuration", error))?;
    // SAFETY: the API filled the buffer with a QUERY_SERVICE_CONFIGW value.
    let config: QUERY_SERVICE_CONFIGW =
        unsafe { std::ptr::read_unaligned(buffer.as_ptr() as *const QUERY_SERVICE_CONFIGW) };
    ServiceStartMode::from_windows(config.dwStartType.0).ok_or_else(|| {
        AdapterError::new(format!(
            "service {service}: unknown start type {}",
            config.dwStartType.0
        ))
    })
}

fn map_status(state: SERVICE_STATUS_CURRENT_STATE) -> ServiceStatus {
    if state == SERVICE_RUNNING {
        ServiceStatus::Running
    } else if state == SERVICE_STOPPED {
        ServiceStatus::Stopped
    } else if state == SERVICE_PAUSED {
        ServiceStatus::Paused
    } else if state == SERVICE_START_PENDING || state == SERVICE_CONTINUE_PENDING {
        ServiceStatus::StartPending
    } else if state == SERVICE_STOP_PENDING || state == SERVICE_PAUSE_PENDING {
        ServiceStatus::StopPending
    } else {
        ServiceStatus::Unknown
    }
}

/// Waits until the service reaches a terminal state or the timeout expires.
///
/// `StartServiceW` and `ControlService` return while the service is still in
/// `*_PENDING`; verifying immediately would report a false failure.
fn wait_for(
    handle: &ScHandle,
    service: &str,
    target: ServiceStatus,
    timeout: Duration,
) -> Result<(), AdapterError> {
    let started = Instant::now();
    loop {
        let status = query_status(handle, service)?;
        if status == target {
            return Ok(());
        }
        if started.elapsed() >= timeout {
            return Err(AdapterError::new(format!(
                "service {service}: did not reach {target:?} within {} s",
                timeout.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn is_missing(error: &windows::core::Error) -> bool {
    // HRESULT_FROM_WIN32(ERROR_SERVICE_DOES_NOT_EXIST)
    error.code().0 as u32 == 0x8007_0424
}

fn service_error(service: &str, action: &str, error: windows::core::Error) -> AdapterError {
    AdapterError::new(format!("service {service}: cannot {action}: {error}"))
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_a_known_service() {
        let services = WindowsServices;
        let state = services.query("Spooler").expect("query");
        assert!(state.is_some(), "Spooler must exist on Windows 10");
        let state = state.expect("state");
        assert!(matches!(
            state.status,
            ServiceStatus::Running | ServiceStatus::Stopped | ServiceStatus::Paused
        ));
    }

    #[test]
    fn missing_service_is_none() {
        let services = WindowsServices;
        assert_eq!(
            services.query("WinTweakerNoSuchService").expect("query"),
            None
        );
    }

    #[test]
    fn start_and_stop_masks_allow_status_queries() {
        // The real start/stop paths need SERVICE_QUERY_STATUS on the same handle,
        // otherwise the wait loop fails with access denied. Open read/write masks
        // without changing the service state and read the status through them.
        let scm = open_scm().expect("service manager");
        for mask in [SERVICE_START, SERVICE_STOP] {
            let handle = open_service(&scm, "Spooler", mask | SERVICE_QUERY_STATUS)
                .unwrap_or_else(|error| panic!("cannot open with mask {mask:#x}: {error}"));
            let status = query_status(&handle, "Spooler").expect("status");
            assert!(matches!(
                status,
                ServiceStatus::Running
                    | ServiceStatus::Stopped
                    | ServiceStatus::Paused
                    | ServiceStatus::StartPending
                    | ServiceStatus::StopPending
                    | ServiceStatus::Unknown
            ));
        }
    }
}
