//! Reports whether processes run elevated, as the dock decides it.
//! Usage: spike_elevated <pid> [pid...]

use nosih_shell::launcher::process_is_elevated;

fn main() {
    for pid in std::env::args()
        .skip(1)
        .filter_map(|arg| arg.parse::<u32>().ok())
    {
        println!("{pid}: elevated={}", process_is_elevated(pid));
    }
}
