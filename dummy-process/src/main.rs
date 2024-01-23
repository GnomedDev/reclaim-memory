use std::time::Duration;

use libc::{c_int, size_t};

fn main() {
    let malloc_trim_ptr: unsafe extern "C" fn(size_t) -> c_int = libc::malloc_trim;
    unsafe { malloc_trim_ptr(0) };

    println!("process id {}", std::process::id());
    println!("malloc_trim is at {:p}", malloc_trim_ptr);

    let _ = std::io::stdin().read_line(&mut String::new());
}
