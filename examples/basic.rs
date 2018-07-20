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
