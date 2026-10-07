//! The symbols an object from another compiler defines, read off its ELF
//! symbol table.
//!
//! An object `ccinrs` made says which C symbols it defines in a mark of its
//! own (see `driver::check_stamp`), which is what a shared library exports.
//! An object GCC, NASM or an assembler made says nothing of the kind, and a
//! shared library linked with one exports its symbols too — libwebp's
//! `libsharpyuv.so`, half of which a build may compile with another
//! compiler. So for those the symbol table is read: every global or weak
//! symbol the object defines with default or protected visibility, as GNU ld
//! would export it. Relocatable objects only, 32- or 64-bit, either byte
//! order, alone or as the members of an archive; anything else has none.

/// The symbols `bytes` — an object, or an archive of them — defines for
/// export, leaving out every object that contains `ours`, which says what it
/// defines itself.
pub fn exported_symbols(bytes: &[u8], ours: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(members) = bytes.strip_prefix(b"!<arch>\n") {
        for member in archive_members(members) {
            if !contains(member, ours) {
                object_symbols(member, &mut names);
            }
        }
    } else if !contains(bytes, ours) {
        object_symbols(bytes, &mut names);
    }
    names
}

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && bytes.windows(needle.len()).any(|w| w == needle)
}

/// The members of an archive, after its magic: each a 60-byte header, whose
/// size field is decimal ASCII, then the data, padded to an even length. The
/// symbol index and the long-name table are members like any other here, and
/// are not objects.
fn archive_members(mut rest: &[u8]) -> Vec<&[u8]> {
    let mut members = Vec::new();
    while rest.len() >= 60 {
        let size = std::str::from_utf8(&rest[48..58])
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok());
        let Some(size) = size else {
            break;
        };
        let Some(data) = rest.get(60..60 + size) else {
            break;
        };
        members.push(data);
        let next = 60 + size + size % 2;
        rest = rest.get(next..).unwrap_or(&[]);
    }
    members
}

/// One ELF relocatable object's defined global and weak symbols.
fn object_symbols(bytes: &[u8], names: &mut Vec<String>) {
    let Some(elf) = Elf::new(bytes) else {
        return;
    };
    // ET_REL: an executable or a shared library exports its own.
    if elf.u16(16) != Some(1) {
        return;
    }
    let (shoff, shentsize, shnum) = if elf.wide {
        (elf.u64(0x28), elf.u16(0x3A), elf.u16(0x3C))
    } else {
        (elf.u32(0x20).map(u64::from), elf.u16(0x2E), elf.u16(0x30))
    };
    let (Some(shoff), Some(shentsize), Some(shnum)) = (shoff, shentsize, shnum) else {
        return;
    };
    let section = |index: u32| -> Option<Section> {
        let at = usize::try_from(shoff).ok()? + index as usize * usize::from(shentsize);
        Some(if elf.wide {
            Section {
                kind: elf.u32(at + 4)?,
                offset: elf.u64(at + 24)?,
                size: elf.u64(at + 32)?,
                link: elf.u32(at + 40)?,
            }
        } else {
            Section {
                kind: elf.u32(at + 4)?,
                offset: u64::from(elf.u32(at + 16)?),
                size: u64::from(elf.u32(at + 20)?),
                link: elf.u32(at + 24)?,
            }
        })
    };
    for index in 0..u32::from(shnum) {
        // SHT_SYMTAB, whose `link` is its string table.
        let Some(symtab) = section(index).filter(|s| s.kind == 2) else {
            continue;
        };
        let Some(strtab) = section(symtab.link) else {
            continue;
        };
        let entry = if elf.wide { 24 } else { 16 };
        let count = symtab.size / entry;
        for i in 0..count {
            let Some(at) = usize::try_from(symtab.offset + i * entry).ok() else {
                break;
            };
            let fields = if elf.wide {
                (elf.u32(at), elf.u8(at + 4), elf.u8(at + 5), elf.u16(at + 6))
            } else {
                (
                    elf.u32(at),
                    elf.u8(at + 12),
                    elf.u8(at + 13),
                    elf.u16(at + 14),
                )
            };
            let (Some(name), Some(info), Some(other), Some(shndx)) = fields else {
                break;
            };
            let binding = info >> 4;
            let kind = info & 0xf;
            let visibility = other & 3;
            // Global or weak; not a section or file symbol; default or
            // protected; defined (not SHN_UNDEF).
            if !matches!(binding, 1 | 2)
                || matches!(kind, 3 | 4)
                || !matches!(visibility, 0 | 3)
                || shndx == 0
            {
                continue;
            }
            if let Some(name) = elf.string(strtab.offset, strtab.size, name)
                && !name.is_empty()
                && !names.iter().any(|n| n == name)
            {
                names.push(name.to_owned());
            }
        }
    }
}

struct Section {
    kind: u32,
    offset: u64,
    size: u64,
    link: u32,
}

/// An ELF file's bytes, its class and its byte order.
struct Elf<'a> {
    bytes: &'a [u8],
    wide: bool,
    big: bool,
}

impl<'a> Elf<'a> {
    fn new(bytes: &'a [u8]) -> Option<Self> {
        if bytes.get(..4)? != b"\x7fELF" {
            return None;
        }
        let wide = match bytes.get(4)? {
            1 => false,
            2 => true,
            _ => return None,
        };
        let big = match bytes.get(5)? {
            1 => false,
            2 => true,
            _ => return None,
        };
        Some(Self { bytes, wide, big })
    }

    fn take<const N: usize>(&self, at: usize) -> Option<[u8; N]> {
        self.bytes.get(at..at.checked_add(N)?)?.try_into().ok()
    }

    fn u8(&self, at: usize) -> Option<u8> {
        self.bytes.get(at).copied()
    }

    fn u16(&self, at: usize) -> Option<u16> {
        let b = self.take(at)?;
        Some(if self.big {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        })
    }

    fn u32(&self, at: usize) -> Option<u32> {
        let b = self.take(at)?;
        Some(if self.big {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        })
    }

    fn u64(&self, at: usize) -> Option<u64> {
        let b = self.take(at)?;
        Some(if self.big {
            u64::from_be_bytes(b)
        } else {
            u64::from_le_bytes(b)
        })
    }

    /// The NUL-terminated string at `index` in the string table at `offset`.
    fn string(&self, offset: u64, size: u64, index: u32) -> Option<&'a str> {
        let start = usize::try_from(offset).ok()?;
        let table = self.bytes.get(start..start + usize::try_from(size).ok()?)?;
        let tail = table.get(index as usize..)?;
        let end = tail.iter().position(|b| *b == 0)?;
        std::str::from_utf8(&tail[..end]).ok()
    }
}
