# reg-watcher(WIP)

Rust binding to MS Windows `RegNotifyChangeKeyValue` API. Work in progress.

## Features

* synchronous and asynchronous API for registry change watching
* [tokio](https://tokio.rs/) stream API

## Usage

```toml
[dependencies]
reg-watcher = "0.1"
```

### Basic usage

```rust
extern crate reg_watcher;
extern crate winreg;

use winreg::{
    RegKey,
    enums::*,
};
use reg_watcher::*;

fn main() {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let reg_key = hklm.open_subkey(r#"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"#).unwrap();
    let res = watch(&reg_key, filter::REG_LEGAL_CHANGE_FILTER, true, Timeout::Milli(60 * 1000)).unwrap();
    println!("{:?}", res);
}

```

### Async

```rust
extern crate reg_watcher;
extern crate winreg;

use std::{
    time::Duration,
    sync::mpsc::channel,
};
use winreg::{
    RegKey,
    enums::*,
};
use reg_watcher::*;

fn main() {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let reg_key = hklm.open_subkey(r#"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"#).unwrap();
    let w = Watcher::new(reg_key, filter::REG_LEGAL_CHANGE_FILTER, true, Duration::from_secs(1));
    let (sender, receiver) = channel();
    let _ = w.watch_async(sender);

    loop {
        let res = receiver.recv().unwrap();
        println!("{:?}", res);
    }
}

```

### Stream

```rust
extern crate futures;
extern crate reg_watcher;
extern crate winreg;
extern crate tokio;

use futures::prelude::*;
use std::{
    time::Duration,
};
use winreg::{
    RegKey,
    enums::*,
};
use reg_watcher::*;

fn main() {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let reg_key = hklm.open_subkey(r#"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"#).unwrap();
    let w = Watcher::new(reg_key, filter::REG_LEGAL_CHANGE_FILTER, true, Duration::from_secs(1));

    let fut = w.for_each(|_| {
        println!("notify");
        Ok(())
    }).map_err(|err| {
        println!("accept error = {:?}", err);
    });

    tokio::run(fut);
}

```