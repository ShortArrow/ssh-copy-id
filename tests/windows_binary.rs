//! The Windows executable runs on a Windows installation without the Visual C++
//! runtime: it imports no DLL of the C runtime, which is linked statically.
#![cfg(windows)]

use std::fs;

fn u16_at(bytes: &[u8], offset: usize) -> usize {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]]) as usize
}

fn u32_at(bytes: &[u8], offset: usize) -> usize {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize
}

/// The file offset of `rva`, from the section that contains it.
fn file_offset(bytes: &[u8], sections: usize, count: usize, rva: usize) -> usize {
    (0..count)
        .map(|i| sections + i * 40)
        .find_map(|section| {
            let start = u32_at(bytes, section + 12);
            let size = u32_at(bytes, section + 8).max(u32_at(bytes, section + 16));
            (start..start + size)
                .contains(&rva)
                .then(|| rva - start + u32_at(bytes, section + 20))
        })
        .expect("RVA inside a section")
}

/// The DLL names in the import table of the PE32+ image `bytes`.
fn imported_dlls(bytes: &[u8]) -> Vec<String> {
    let pe = u32_at(bytes, 0x3c);
    assert_eq!(&bytes[pe..pe + 4], b"PE\0\0");
    let coff = pe + 4;
    let count = u16_at(bytes, coff + 2);
    let optional = coff + 20;
    assert_eq!(u16_at(bytes, optional), 0x20b, "PE32+ image");
    let sections = optional + u16_at(bytes, coff + 16);
    let import_rva = u32_at(bytes, optional + 112 + 8);
    let mut descriptor = file_offset(bytes, sections, count, import_rva);
    let mut names = Vec::new();
    while bytes[descriptor..descriptor + 20].iter().any(|&b| b != 0) {
        let name = file_offset(bytes, sections, count, u32_at(bytes, descriptor + 12));
        let end = name + bytes[name..].iter().position(|&b| b == 0).unwrap();
        names.push(String::from_utf8_lossy(&bytes[name..end]).to_ascii_lowercase());
        descriptor += 20;
    }
    names
}

#[test]
fn b01_executable_imports_no_c_runtime_dll() {
    let bytes = fs::read(env!("CARGO_BIN_EXE_ssh-copy-id")).unwrap();
    let dlls = imported_dlls(&bytes);
    assert!(dlls.iter().any(|dll| dll == "kernel32.dll"), "{dlls:?}");
    let runtime: Vec<_> = dlls
        .iter()
        .filter(|dll| dll.starts_with("vcruntime") || dll.starts_with("api-ms-win-crt-"))
        .collect();
    assert!(runtime.is_empty(), "C runtime imports: {runtime:?}");
}
