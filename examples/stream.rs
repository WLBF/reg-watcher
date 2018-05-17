extern crate futures;
extern crate reg_watcher;
extern crate winreg;
extern crate tokio_core;

use futures::prelude::*;
use std::{
    time::Duration,
};
use tokio_core::reactor::Core;
use winreg::{
    RegKey,
    enums::*,
};
use reg_watcher::{
    filter,
    Watcher,
};

fn main() {
    let mut reactor = Core::new().unwrap();

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let reg_key = hklm.open_subkey(r#"SOFTWARE\Microsoft\Windows\CurrentVersion"#).unwrap();
    let w = Watcher::new(reg_key, filter::REG_LEGAL_CHANGE_FILTER, true, Duration::from_secs(1));


    let fut = w.for_each(|notify| {
        println!("{:?}", notify);
        Ok(())
    });

    let _ret = reactor.run(fut).unwrap();
}
