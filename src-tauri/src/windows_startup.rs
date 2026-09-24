//! Serialize Windows cold starts until the single-instance receiver exists.
//! The plugin owns the lifetime mutex; this guard only covers initialization.
use std::{marker::PhantomData, rc::Rc};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0},
        System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject, INFINITE},
    },
};

pub(crate) struct StartupGate {
    handle: HANDLE,
    // A Windows mutex must be released on the thread that acquired it.
    _thread: PhantomData<Rc<()>>,
}

impl StartupGate {
    pub(crate) fn acquire(name: &str) -> windows::core::Result<Self> {
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        // SAFETY: name is terminated and live for the call. The handle is
        // closed on every error path and owned by this guard after acquisition.
        let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr()))? };
        match unsafe { WaitForSingleObject(handle, INFINITE) } {
            WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Self {
                handle,
                _thread: PhantomData,
            }),
            _ => {
                let error = windows::core::Error::from_win32();
                let _ = unsafe { CloseHandle(handle) };
                Err(error)
            }
        }
    }
}

impl Drop for StartupGate {
    fn drop(&mut self) {
        // SAFETY: this guard owns the handle and cannot move to another thread.
        unsafe {
            let _ = ReleaseMutex(self.handle);
            let _ = CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn name(case: &str) -> String {
        format!("Local\\bjut-startup-test-{}-{case}", std::process::id())
    }

    #[test]
    fn concurrent_start_waits_until_the_first_receiver_is_ready() {
        let name = name("concurrent");
        let first = StartupGate::acquire(&name).unwrap();
        let (ready, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _second = StartupGate::acquire(&name).unwrap();
            ready.send(()).unwrap();
        });
        assert!(receiver
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_err());
        drop(first);
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn abandoned_startup_does_not_block_the_next_launch() {
        let name = name("abandoned");
        let worker_name = name.clone();
        let abandoned = std::thread::spawn(move || {
            let guard = StartupGate::acquire(&worker_name).unwrap();
            let handle = guard.handle.0 as usize;
            std::mem::forget(guard);
            handle
        })
        .join()
        .unwrap();
        let next = StartupGate::acquire(&name).unwrap();
        drop(next);
        // The test deliberately kept this handle open after its owner exited.
        unsafe {
            CloseHandle(HANDLE(abandoned as *mut _)).unwrap();
        }
    }
}
