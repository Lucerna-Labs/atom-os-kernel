//! Host-side preflight using the exact ELF parser executed by the kernel.
extern crate alloc;
#[allow(dead_code)]
#[path = "../kernel-kit/src/elf.rs"]
mod elf;

fn main() -> std::process::ExitCode {
    let paths: Vec<_> = std::env::args_os().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: check-elf PROGRAM [PROGRAM ...]");
        return std::process::ExitCode::from(2);
    }
    let mut failed = false;
    for path in paths {
        let path = std::path::Path::new(&path);
        let result = std::fs::read(path).map_err(|e| e.to_string()).and_then(|bytes| {
            elf::Image::parse(&bytes).map(|image| (image.entry, image.end - image.start))
                .map_err(|e| format!("kernel loader rejected {e:?}"))
        });
        match result {
            Ok((entry, span)) => println!("ELF_OK {} entry={entry:#x} span={span}", path.display()),
            Err(error) => { eprintln!("ELF_REJECTED {}: {error}", path.display()); failed = true; }
        }
    }
    if failed { std::process::ExitCode::FAILURE } else { std::process::ExitCode::SUCCESS }
}
