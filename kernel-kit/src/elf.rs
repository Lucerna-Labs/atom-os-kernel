#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64_Ehdr {
    pub e_ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64_Phdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

impl Elf64_Ehdr {
    pub fn is_valid(&self) -> bool {
        self.e_ident[0] == 0x7f &&
        self.e_ident[1] == b'E' &&
        self.e_ident[2] == b'L' &&
        self.e_ident[3] == b'F' &&
        self.e_ident[4] == 2 && self.e_ident[5] == 1 && self.e_ident[6] == 1 &&
        self.e_type == 2 && self.e_machine == 62 && self.e_version == 1 &&
        self.e_ehsize == 64 && self.e_phentsize == 56
    }
}

pub const IMAGE_BASE: u64 = 0xffff_ffff_8010_0000;
pub const IMAGE_LIMIT: u64 = IMAGE_BASE + 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfError { Header, Bounds, Segment, Permissions, Entry }

pub struct Image<'a> {
    pub entry: u64,
    pub start: u64,
    pub end: u64,
    pub segments: alloc::vec::Vec<Elf64_Phdr>,
    pub bytes: &'a [u8],
}

impl<'a> Image<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ElfError> {
        if bytes.len() < 64 { return Err(ElfError::Header); }
        let h = unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Elf64_Ehdr) };
        if !h.is_valid() || h.e_phnum == 0 || h.e_phnum > 16 { return Err(ElfError::Header); }
        let phoff = usize::try_from(h.e_phoff).map_err(|_| ElfError::Bounds)?;
        let end = phoff.checked_add(h.e_phnum as usize * 56).ok_or(ElfError::Bounds)?;
        if phoff < 64 || end > bytes.len() { return Err(ElfError::Bounds); }
        let mut result = Self { entry: h.e_entry, start: u64::MAX, end: 0,
            segments: alloc::vec::Vec::new(), bytes };
        for i in 0..h.e_phnum as usize {
            let p = unsafe { core::ptr::read_unaligned(bytes.as_ptr().add(phoff + 56 * i) as *const Elf64_Phdr) };
            if p.p_type != 1 { continue; }
            if p.p_memsz == 0 { continue; }
            let vend = p.p_vaddr.checked_add(p.p_memsz).ok_or(ElfError::Bounds)?;
            let fend = p.p_offset.checked_add(p.p_filesz).ok_or(ElfError::Bounds)?;
            if p.p_vaddr < IMAGE_BASE || vend > IMAGE_LIMIT || p.p_filesz > p.p_memsz || fend > bytes.len() as u64 {
                return Err(ElfError::Bounds);
            }
            if p.p_align > 1 && (!p.p_align.is_power_of_two() || p.p_vaddr % p.p_align != p.p_offset % p.p_align) {
                return Err(ElfError::Segment);
            }
            if p.p_flags & !7 != 0 || p.p_flags & 4 == 0 || p.p_flags & 3 == 3 {
                return Err(ElfError::Permissions);
            }
            for old in &result.segments {
                if p.p_vaddr < old.p_vaddr + old.p_memsz && old.p_vaddr < vend { return Err(ElfError::Segment); }
            }
            result.start = result.start.min(p.p_vaddr & !4095);
            result.end = result.end.max((vend + 4095) & !4095);
            result.segments.push(p);
        }
        if result.segments.is_empty() || !result.segments.iter().any(|p|
            p.p_flags & 1 != 0 && h.e_entry >= p.p_vaddr && h.e_entry < p.p_vaddr + p.p_memsz) {
            return Err(ElfError::Entry);
        }
        for page in (result.start..result.end).step_by(4096) {
            if result.page_flags(page) & 3 == 3 { return Err(ElfError::Permissions); }
        }
        Ok(result)
    }

    pub fn page_flags(&self, page: u64) -> u32 {
        self.segments.iter().filter(|p| p.p_vaddr < page + 4096 && p.p_vaddr + p.p_memsz > page)
            .fold(0, |flags, p| flags | p.p_flags)
    }
}
