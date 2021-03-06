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
