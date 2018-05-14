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
use reg_watcher::{
    filter,
    Watcher,
};

fn main() {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let reg_key = hklm.open_subkey(r#"SOFTWARE\Microsoft\Windows\CurrentVersion"#).unwrap();
    let mut w = Watcher::new(reg_key, filter::REG_LEGAL_CHANGE_FILTER, true, Duration::from_secs(1));
    let (sender, receiver) = channel();
    let _ = w.watch_async(sender);

    loop {
        let resp = receiver.recv().unwrap();
        println!("{:?}", resp);
    }

}