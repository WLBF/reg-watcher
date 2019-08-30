//!Rust binding to MS Windows `RegNotifyChangeKeyValue` API. Work in progress.
//!
//!## Features
//!
//!* synchronous and asynchronous API for registry change watching
//!* [Tokio](https://tokio.rs/) stream API
//!
//!## Usage
//!
//!```toml,ignore
//![dependencies]
//!reg-watcher = "0.1"
//!```
//!
//!### Basic usage
//!
//!```no_run
//!extern crate reg_watcher;
//!extern crate winreg;
//!
//!use winreg::{
//!    RegKey,
//!    enums::*,
//!};
//!use reg_watcher::*;
//!
//!fn main() {
//!    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
//!    let reg_key = hklm.open_subkey(r#"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"#).unwrap();
//!    let res = watch(&reg_key, filter::REG_LEGAL_CHANGE_FILTER, true, Timeout::Milli(60 * 1000)).unwrap();
//!    println!("{:?}", res);
//!}
//!```
//!
//!### Async
//!
//!```no_run
//!extern crate reg_watcher;
//!extern crate winreg;
//!
//!use std::{
//!    time::Duration,
//!    sync::mpsc::channel,
//!};
//!use winreg::{
//!    RegKey,
//!    enums::*,
//!};
//!use reg_watcher::*;
//!
//!fn main() {
//!    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
//!    let reg_key = hklm.open_subkey(r#"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"#).unwrap();
//!    let w = Watcher::new(reg_key, filter::REG_LEGAL_CHANGE_FILTER, true, Duration::from_secs(1));
//!    let (sender, receiver) = channel();
//!    let _ = w.watch_async(sender);
//!
//!    loop {
//!        let res = receiver.recv().unwrap();
//!        println!("{:?}", res);
//!    }
//!}
//!```
//!
//!### Stream
//!
//!```no_run
//!extern crate futures;
//!extern crate reg_watcher;
//!extern crate winreg;
//!extern crate tokio;
//!
//!use futures::prelude::*;
//!use std::{
//!    time::Duration,
//!};
//!use winreg::{
//!    RegKey,
//!    enums::*,
//!};
//!use reg_watcher::*;
//!
//!fn main() {
//!    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
//!    let reg_key = hklm.open_subkey(r#"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"#).unwrap();
//!    let w = Watcher::new(reg_key, filter::REG_LEGAL_CHANGE_FILTER, true, Duration::from_secs(1));
//!
//!    let fut = w.for_each(|_| {
//!        println!("notify");
//!        Ok(())
//!    }).map_err(|err| {
//!        println!("accept error = {:?}", err);
//!    });
//!
//!    tokio::run(fut);
//!}
//!```


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

/// Reexport notify filters
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

/// Timeout value for `watch` function
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

/// Watching response returned by `watch`
#[derive(Debug)]
pub enum WatchResponse {
    Notify,
    Timeout,
}

/// Watch a specific registry key.
/// Block the thread until the changing notify occur or timeout expired.
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

/// Watcher for asynchronous watching
/// Also can be used as a [Tokio](https://tokio.rs/) stream.
pub struct Watcher {
    reg_key: Option<RegKey>,
    notify_filter: u32,
    watch_subtree: bool,
    tick_duration: Duration,
    handle: Option<thread::JoinHandle<()>>,
    stream_receiver: Option<Receiver<WatchResponse>>
}

impl Watcher {
    /// return a new `watcher`
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

    /// Create a external thread to Watch a specific registry key.
    /// Pass `WatchResponse` by predefined `Sender` 
    pub fn watch_async(
        &mut self,
        sender: Sender<WatchResponse>) -> Result<(), Error>
    {
        let builder = thread::Builder::new().name("reg-watcher".into());
        let reg_key = self.reg_key.take().ok_or(format_err!("watcher none registry key"))?;
        let notify_filter = self.notify_filter;
        let watch_subtree = self.watch_subtree;
        let tick_duration = self.tick_duration;

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
                    Ok(v) => sender.send(v)
                        .unwrap_or_else(|e| panic!("invalid sender: {:?}", e)),
                };
                thread::sleep(tick_duration);
            }
        })?;
        self.handle = Some(handle);
        Ok(())
    }
}

/// [Stream](https://docs.rs/futures/0.1.23/futures/stream/trait.Stream.html) implemention for `Watcher`
impl Stream for Watcher {
    type Item = WatchResponse;
    type Error = Error;

    fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
        match (self.reg_key.is_some(), self.handle.is_some()) {
            // start external watching thread at first poll
            (true, false) => {
                let builder = thread::Builder::new().name("reg-watcher".into());
                let reg_key = self.reg_key.take().unwrap(); // `None` already handled before
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
                    // when get polled again, try to receive `WatchRespone` by calling `try_recv`
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
