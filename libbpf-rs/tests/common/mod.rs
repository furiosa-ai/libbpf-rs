use std::error::Error;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use goblin::elf::sym;
use goblin::elf::Elf;
use libbpf_rs::Map;
use libbpf_rs::MapCore;
use libbpf_rs::MapMut;
use libbpf_rs::Object;
use libbpf_rs::ObjectBuilder;
use libbpf_rs::OpenObject;
use libbpf_rs::ProgramMut;


pub fn get_test_object_path(filename: &str) -> PathBuf {
    let mut path = PathBuf::new();
    // env!() macro fails at compile time if var not found
    path.push(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/bin");
    path.push(filename);
    path
}

pub fn open_test_object(filename: &str) -> OpenObject {
    let obj_path = get_test_object_path(filename);
    let obj = ObjectBuilder::default()
        .debug(true)
        .open_file(obj_path)
        .expect("failed to open object");
    obj
}

pub fn get_test_object(filename: &str) -> Object {
    open_test_object(filename)
        .load()
        .expect("failed to load object")
}

/// Find the BPF map with the given name, panic if it does not exist.
#[track_caller]
pub fn get_map<'obj>(object: &'obj Object, name: &str) -> Map<'obj> {
    object
        .maps()
        .find(|map| map.name() == name)
        .unwrap_or_else(|| panic!("failed to find map `{name}`"))
}

/// Find the BPF map with the given name, panic if it does not exist.
#[track_caller]
pub fn get_map_mut<'obj>(object: &'obj mut Object, name: &str) -> MapMut<'obj> {
    object
        .maps_mut()
        .find(|map| map.name() == name)
        .unwrap_or_else(|| panic!("failed to find map `{name}`"))
}

/// Find the BPF program with the given name, panic if it does not exist.
#[track_caller]
pub fn get_prog_mut<'obj>(object: &'obj mut Object, name: &str) -> ProgramMut<'obj> {
    object
        .progs_mut()
        .find(|map| map.name() == name)
        .unwrap_or_else(|| panic!("failed to find program `{name}`"))
}

/// A helper function for instantiating a `RingBuffer` with a callback meant to
/// be invoked when `action` is executed and that is intended to trigger a write
/// to said `RingBuffer` from kernel space, which then reads a single `i32` from
/// this buffer from user space and returns it.
pub fn with_ringbuffer<F>(map: &Map, action: F) -> i32
where
    F: FnOnce(),
{
    let mut value = 0i32;
    {
        let callback = |data: &[u8]| {
            plain::copy_from_bytes(&mut value, data).expect("Wrong size");
            0
        };

        let mut builder = libbpf_rs::RingBufferBuilder::new();
        builder.add(map, callback).expect("failed to add ringbuf");
        let mgr = builder.build().expect("failed to build");

        action();
        mgr.consume().expect("failed to consume ringbuf");
    }

    value
}

pub fn get_symbol_offset(binary_path: &Path, symbol_name: &str) -> Result<usize, Box<dyn Error>> {
    let buffer = fs::read(binary_path)?;
    let elf = Elf::parse(&buffer)?;

    // Check dynamic symbols
    for sym in elf.dynsyms.iter() {
        if sym.st_type() != sym::STT_FUNC {
            continue;
        }
        if let Some(name) = elf.dynstrtab.get_at(sym.st_name) {
            if name == symbol_name {
                let sec = &elf.section_headers[sym.st_shndx];
                let offset = sym.st_value - sec.sh_addr + sec.sh_offset;
                return Ok(offset as usize);
            }
        }
    }

    // Check regular symbols
    for sym in elf.syms.iter() {
        if sym.st_type() != sym::STT_FUNC {
            continue;
        }
        if let Some(name) = elf.strtab.get_at(sym.st_name) {
            if name == symbol_name {
                let sec = &elf.section_headers[sym.st_shndx];
                let offset = sym.st_value - sec.sh_addr + sec.sh_offset;
                return Ok(offset as usize);
            }
        }
    }

    Err(format!(
        "Symbol `{}` not found in binary {:?}",
        symbol_name, binary_path
    )
    .into())
}
