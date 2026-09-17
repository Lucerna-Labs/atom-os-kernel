use kernel_kit::address_space::{AddressSpace, MapError, frames_allocate, STACK_TOP, STACK_BYTES, RECV_BASE};
use kernel_kit::context::Context;
use kernel_kit::elf::Image;
use kernel_kit::paging::phys_to_virt;
use kernel_kit::trap::TrapFrame;

pub fn load_image(name: &str, kernel_root: u64) -> Result<(AddressSpace, u64), MapError> {
    let fs = kernel_kit::fs::ROOT_FS.lock();
    let result = (|| {
        let bytes = fs.file(name).ok_or(MapError::Address)?;
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

pub fn create(pid: usize, parent: usize, name: &str, kernel_root: u64) -> Result<Context, MapError> {
    let (space, entry) = load_image(name, kernel_root)?;
    let pages = 4;
    let phys = frames_allocate(pages)?;
    let top = phys_to_virt(phys) + pages as u64 * 4096;
    let rsp = top - core::mem::size_of::<TrapFrame>() as u64;
    unsafe { *(rsp as *mut TrapFrame) = TrapFrame::new_user(entry, STACK_TOP); reset_fpu(rsp); }
    let mut context = Context::new(pid, rsp, top, space.root);
    context.parent = parent;
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
