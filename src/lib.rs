#[macro_use]
extern crate failure;
extern crate futures;
extern crate uuid;
extern crate widestring;
extern crate winapi;
extern crate winreg;

use std::{
    ptr,
    time::Duration,
    thread,
    sync::mpsc::{channel, Receiver, Sender, TryRecvError},
};
use failure::Error;
use winapi::{
    um::{
        winnt::*,
        winreg::*,
        synchapi::{CreateEventW, WaitForSingleObject},
        handleapi::CloseHandle,
        winbase::*,
        errhandlingapi::GetLastError,
    },
};
use winapi::shared::winerror::*;
use winreg::RegKey;
use widestring::WideCString;
use uuid::Uuid;
use futures::{
    stream::Stream,
    prelude::*,
};

pub mod filter {
    pub use winapi::um::winnt::{
        REG_NOTIFY_CHANGE_NAME,
        REG_NOTIFY_CHANGE_ATTRIBUTES,
        REG_NOTIFY_CHANGE_LAST_SET,
        REG_NOTIFY_CHANGE_SECURITY,
        REG_NOTIFY_THREAD_AGNOSTIC,
        REG_LEGAL_CHANGE_FILTER,
    };
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
        let handle = unsafe { CreateEventW(ptr::null_mut(), false as i32, true as i32, name_ptr) };
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

#[derive(Debug)]
pub enum WatchResponse {
    Notify,
    Timeout,
}

pub fn watch(
    reg_key: &RegKey,
    notify_filter: u32,
    watch_subtree: bool,
    timeout: Timeout,
) -> Result<WatchResponse, Error> {

    // generate unique name for wait event
    let uid = Uuid::new_v4().hyphenated().to_string() + "-reg-watcher";
    let name = WideCString::from_str(uid)?;

    let time_num = match &timeout {
        &Timeout::Milli(v) => v,
        &Timeout::Infinite => INFINITE,
    };

    let wait_handle = WaitEvent::create(name.as_ptr());

    unsafe {
        let ret = RegNotifyChangeKeyValue(
            reg_key.raw_handle(),
            watch_subtree as i32,
            notify_filter,
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
            WAIT_FAILED => Err(format_err!(
                "WaitForSingleObject return code: {}",
                GetLastError()
            )),
            _ => unreachable!(),
        }
    }
}

pub struct Watcher {
    reg_key: Option<RegKey>,
    notify_filter: u32,
    watch_subtree: bool,
    tick_duration: Duration,
    handle: Option<thread::JoinHandle<()>>,
    stream_receiver: Option<Receiver<WatchResponse>>
}

impl Watcher {
    pub fn new(
        reg_key: RegKey,
        notify_filter: u32,
        watch_subtree: bool,
        tick_duration: Duration) -> Self
    {
        Self {
            reg_key: Some(reg_key),
            notify_filter,
            watch_subtree,
            tick_duration,
            handle: None,
            stream_receiver: None,
        }
    }

    pub fn watch_async(
        mut self,
        sender: Sender<WatchResponse>) -> Result<(), Error>
    {
        let builder = thread::Builder::new().name("reg-watcher".into());
        let reg_key = self.reg_key.take().ok_or(format_err!("watcher none registry key"))?;
        
        //start the external watching thread
        let handle = builder.spawn(move || {
            loop {
                match watch(
                    &reg_key,
                    self.notify_filter,
                    self.watch_subtree,
                    Timeout::Infinite,
                ) {
                    Err(e) => panic!("call watcher.watch err: {:?}", e),
                    Ok(v) => sender.send(v)
                        .unwrap_or_else(|e| panic!("invalid sender: {:?}", e)),
                };
                thread::sleep(self.tick_duration);
            }
        })?;
        self.handle = Some(handle);
        Ok(())
    }
}

impl Stream for Watcher {
    type Item = WatchResponse;
    type Error = Error;

    fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
        match (self.reg_key.is_some(), self.handle.is_some()) {
            // start external watching thread at first poll
            (true, false) => {
                let builder = thread::Builder::new().name("reg-watcher".into());
                let reg_key = self.reg_key.take().unwrap(); // 'None' already handled before
                let notify_filter = self.notify_filter;
                let watch_subtree = self.watch_subtree;
                let tick_duration = self.tick_duration;
                let (sender, receiver) = channel();
                self.stream_receiver = Some(receiver);

                // get current task handle
                let task_handle = futures::task::current();

                //start the external watching thread
                let handle = builder.spawn(move || {
                    loop {
                        match watch(
                            &reg_key,
                            notify_filter,
                            watch_subtree,
                            Timeout::Infinite,
                        ) {
                            Err(e) => panic!("call watcher.watch err: {:?}", e),
                            Ok(v) => {
                                // send the response
                                sender.send(v).unwrap_or_else(|e| panic!("invalid sender: {:?}", e));
                                //notify the executor to poll this task
                                task_handle.notify()
                            }, 
                        };
                        thread::sleep(tick_duration);
                    }
                })?;
                self.handle = Some(handle);
                Ok(Async::NotReady)
            }
            (false, true) => {
                if let Some(ref rx) = self.stream_receiver {
                    match rx.try_recv() {
                        Ok(v) => Ok(Async::Ready(Some(v))),
                        Err(TryRecvError::Empty) => Ok(Async::NotReady),
                        Err(e) => Err(format_err!("stream_receiver try_recv: {}", e)),
                    }
                } else {
                    unreachable!()
                }
            }
            (_, _) => Err(format_err!("watcher thread does not exist")),
        }
    }
}
