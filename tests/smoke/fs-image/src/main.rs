use easy_fs::{BlockDevice, EasyFileSystem};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    sync::{Arc, Mutex},
};
struct Disk(Mutex<File>);
impl BlockDevice for Disk {
    fn read_block(&self, id: usize, buf: &mut [u8]) {
        let mut disk = self.0.lock().unwrap();
        disk.seek(SeekFrom::Start(id as u64 * 512)).unwrap();
        disk.read_exact(buf).unwrap();
    }
    fn write_block(&self, id: usize, buf: &[u8]) {
        let mut disk = self.0.lock().unwrap();
        disk.seek(SeekFrom::Start(id as u64 * 512)).unwrap();
        disk.write_all(buf).unwrap();
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&args[1])
        .unwrap();
    file.set_len(4096 * 512).unwrap();
    let disk: Arc<dyn BlockDevice> = Arc::new(Disk(Mutex::new(file)));
    let fs = EasyFileSystem::create(disk, 4096, 1);
    let root = EasyFileSystem::root_inode(&fs);
    for pair in args[2..].chunks_exact(2) {
        let bytes = std::fs::read(&pair[1]).unwrap();
        let inode = root.create(&pair[0]).unwrap();
        assert_eq!(inode.write_at(0, &bytes), bytes.len());
    }
}
