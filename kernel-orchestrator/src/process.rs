use kernel_kit::address_space::{AddressSpace, MapError, frames_allocate, STACK_TOP, STACK_BYTES, RECV_BASE};
use kernel_kit::context::Context;
use kernel_kit::elf::Image;
use kernel_kit::paging::phys_to_virt;
use kernel_kit::trap::TrapFrame;

/// Finds a program: a path with a `/` as given, otherwise `/bin/<name>` and then
/// `/<name>`.
pub fn find_program(fs: &kernel_kit::fs::Fs, name: &str) -> Option<kernel_kit::fs::Ino> {
    if name.contains('/') { return fs.resolve(name).ok(); }
    let bin = alloc::format!("/{}/{}", kernel_kit::fs::BIN, name);
    fs.resolve(&bin).or_else(|_| fs.resolve(name)).ok()
}

/// Largest program image read from a saved file.
const PROGRAM_MAX: u64 = 64 << 20;

pub fn load_image(name: &str, kernel_root: u64) -> Result<(AddressSpace, u64), MapError> {
    let fs = kernel_kit::fs::ROOT_FS.lock();
    let result = (|| {
        let ino = find_program(fs, name).ok_or(MapError::Address)?;
        kernel_kit::storage::ensure_loaded(fs, ino).map_err(|_| MapError::Address)?;
        let file = fs.node(ino).and_then(|n| n.file()).ok_or(MapError::Address)?;
        let owned;
        let bytes: &[u8] = match &file.content {
            kernel_kit::fs::Content::Builtin(bytes) => bytes,
            kernel_kit::fs::Content::Data(data) if data.len() <= PROGRAM_MAX => { owned = data.to_vec(); &owned }
            _ => return Err(MapError::Address),
        };
        let image = Image::parse(bytes).map_err(|_| MapError::Address)?;
        let mut space = AddressSpace::new(kernel_root)?;
        let pages = ((image.end - image.start) / 4096) as usize;
        let phys = space.allocate_region(image.start, pages, false)?;
        for segment in &image.segments {
            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr().add(segment.p_offset as usize),
                    (phys_to_virt(phys) + segment.p_vaddr - image.start) as *mut u8, segment.p_filesz as usize);
            }
        }
        for page in (image.start..image.end).step_by(4096) {
            let flags = image.page_flags(page);
            space.map_page(page, phys + page - image.start, flags & 2 != 0, flags & 1 != 0)?;
        }
        space.allocate_region(STACK_TOP - STACK_BYTES as u64, STACK_BYTES / 4096, false)?;
        space.allocate_region(RECV_BASE, 1, false)?;
        Ok((space, image.entry))
    })();
    kernel_kit::fs::ROOT_FS.unlock();
    result
}

/// `arguments` is the packed argv (see `kernel_kit::arguments::pack`).
pub fn create(pid: usize, parent: usize, name: &str, arguments: alloc::vec::Vec<u8>, kernel_root: u64) -> Result<Context, MapError> {
    let (space, entry) = load_image(name, kernel_root)?;
    let pages = 4;
    let phys = frames_allocate(pages)?;
    let top = phys_to_virt(phys) + pages as u64 * 4096;
    let rsp = top - core::mem::size_of::<TrapFrame>() as u64;
    unsafe { *(rsp as *mut TrapFrame) = TrapFrame::new_user(entry, STACK_TOP); reset_fpu(rsp); }
    let mut context = Context::new(pid, rsp, top, space.root);
    context.parent = parent;
    context.set_name(alloc::string::String::from(name));
    context.arguments = arguments;
    context.space = Some(space);
    context.kernel_stack_phys = phys;
    context.kernel_stack_pages = pages;
    Ok(context)
}

/// The interrupt wrappers keep an aligned FXSAVE area immediately below the
/// integer trap frame. Initial/exec state must match their restore layout.
pub unsafe fn reset_fpu(rsp: u64) {
    let fx = ((rsp - 512) & !15) as *mut u8;
    core::ptr::write_bytes(fx, 0, 512);
    *(fx as *mut u16) = 0x037f;
    *(fx.add(24) as *mut u32) = 0x1f80;
}
