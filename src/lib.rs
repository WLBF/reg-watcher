extern crate winapi;
extern crate winreg;
#[macro_use]
extern crate failure;
extern crate widestring;
extern crate uuid;

use std::ptr;
use std::time::Duration;
use std::thread;
use std::sync::mpsc::{Sender, Receiver, channel};
use failure::Error;
use winapi::um::winnt::*;
use winapi::um::winreg::*;
use winapi::um::synchapi::{CreateEventW, WaitForSingleObject};
use winapi::um::handleapi::CloseHandle;
use winapi::um::winbase::*;
use winapi::shared::winerror::*;
use winapi::um::errhandlingapi::GetLastError;
use winreg::RegKey;
use widestring::WideCString;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub enum NotifyFilter {
    ChangeName = 0x00000001,
    ChangeAttributes = 0x00000002,
    ChangeLastSet = 0x00000004,
    ChangeSecurity = 0x00000008,
    ThreadAgnostic = 0x10000000,
}

pub enum Timeout {
    Milli(u32),
    Infinite,
}

struct WaitEvent {
    handle: HANDLE,
}

impl WaitEvent {
    pub fn create(name_ptr: LPCWSTR) -> Self {
        let handle = unsafe {
            CreateEventW(
                ptr::null_mut(),
                false as i32,
                true as i32,
                name_ptr,
            )
        };
        Self { handle }
    }

    pub fn handle(&self) -> HANDLE {
        self.handle
    }
}

impl Drop for WaitEvent {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

pub enum WatchResponse {
    Notify,
    Timeout,
}

pub fn watch(
    reg_key: RegKey,
    notify_filter: NotifyFilter,
    watch_subtree: bool,
    timeout: Timeout) -> Result<WatchResponse, Error>
{
    let uid = Uuid::new_v4().hyphenated().to_string();
    let name = WideCString::from_str(uid)?;

    let time_num = match &timeout {
        &Timeout::Milli(v) => v,
        &Timeout::Infinite => INFINITE,
    };

    let wait_handle = WaitEvent::create(name.as_ptr());

    unsafe {
        let ret = RegNotifyChangeKeyValue(
            reg_key.handle(),
            watch_subtree as i32,
            notify_filter as u32,
            wait_handle.handle(),
            true as i32,
        );

        if ret != ERROR_SUCCESS as i32 {
            Err(format_err!("RegNotifyChangeKeyValue return code: {}", ret))?
        }

        match WaitForSingleObject(wait_handle.handle(), time_num) {
            WAIT_ABANDONED => Err(format_err!("WaitForSingleObject return WAIT_ABANDONED")),
            WAIT_OBJECT_0 => Ok(WatchResponse::Notify),
            WAIT_TIMEOUT => Ok(WatchResponse::Timeout),
            WAIT_FAILED => Err(format_err!("WaitForSingleObject return code: {}", GetLastError())),
            _ => unreachable!(),
        }
    }
}

pub struct Watcher {
    reg_key: RegKey,
    notify_filter: NotifyFilter,
    watch_subtree: bool,
    tick_duration: Duration,
    sender: Sender<WatchResponse>,
}

impl Watcher {
    pub fn new(
        reg_key: RegKey,
        notify_filter: NotifyFilter,
        watch_subtree: bool,
        tick_duration: Duration,
        sender: Sender<WatchResponse>) -> Self
    {
        Self {
            reg_key,
            notify_filter,
            watch_subtree,
            tick_duration,
            sender,
        }
    }

    pub fn watch_async(self) -> Result<(), Error>
    {
        let builder = thread::Builder::new().name("reg-watcher".into());
        let _handler = builder.spawn(move || {
            loop {
                match watch(
                    self.reg_key.clone(),
                    self.notify_filter,
                    self.watch_subtree,
                    Timeout::Infinite,
                ) {
                    Err(e) => panic!("call watcher.watch err: {:?}", e),
                    Ok(v) => self.sender.send(v)
                        .unwrap_or_else(|e| panic!("invalid sender: {:?}", e)),
                }
                thread::sleep(self.tick_duration);
            }
        })?;
        Ok(())
    }
}