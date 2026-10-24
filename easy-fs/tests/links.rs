use easy_fs::{BlockDevice, EasyFileSystem};
use std::sync::{Arc, Mutex};
struct MemoryDevice(Mutex<Vec<[u8; 512]>>);
impl BlockDevice for MemoryDevice {
    fn read_block(&self, id: usize, buffer: &mut [u8]) {
        buffer.copy_from_slice(&self.0.lock().unwrap()[id]);
    }
    fn write_block(&self, id: usize, buffer: &[u8]) {
        self.0.lock().unwrap()[id].copy_from_slice(buffer);
    }
}
#[test]
fn hard_links_open_unlink_reclamation_and_directory_slot_reuse() {
    let device: Arc<dyn BlockDevice> = Arc::new(MemoryDevice(Mutex::new(vec![[0; 512]; 4096])));
    let fs = EasyFileSystem::create(device.clone(), 4096, 1);
    let root = EasyFileSystem::root_inode(&fs);
    let file = root.create("original").unwrap();
    let ino = file.stat().0;
    assert_eq!(file.stat().2, 1);
    assert_eq!(file.write_at(0, b"hello"), 5);
    assert_eq!(root.link("original", "alias"), 0);
    let alias = root.find("alias").unwrap();
    assert!(Arc::ptr_eq(&file, &alias));
    assert_eq!(alias.stat(), (ino, false, 2));
    assert_eq!(root.link("missing", "other"), -1);
    assert_eq!(root.link("original", "alias"), -1);
    assert_eq!(root.link("original", "original"), -1);
    assert_eq!(root.link("original", ""), -1);
    assert_eq!(root.link("original", "a/b"), -1);
    assert_eq!(root.link("original", &"x".repeat(28)), -1);
    assert_eq!(root.unlink("original"), 0);
    assert!(root.find("original").is_none());
    assert_eq!(alias.stat().2, 1);
    assert_eq!(root.unlink("alias"), 0);
    assert_eq!(alias.stat().2, 0);
    let mut data = [0; 5];
    assert_eq!(alias.read_at(0, &mut data), 5);
    assert_eq!(&data, b"hello");
    let other = root.create("other").unwrap();
    assert_ne!(other.stat().0, ino);
    drop(file);
    drop(alias);
    let reused = root.create("reused").unwrap();
    assert_eq!(reused.stat().0, ino);
    assert_eq!(reused.read_at(0, &mut data), 0);
    for i in 0..80 {
        root.create(&format!("entry{i}")).unwrap();
    }
    for i in 0..80 {
        assert_eq!(root.unlink(&format!("entry{i}")), 0);
    }
    assert_eq!(root.ls().len(), 2);
    for i in 0..80 {
        root.create(&format!("again{i}")).unwrap();
    }
    assert_eq!(root.ls().len(), 82);
    assert_eq!(root.link("reused", "persisted"), 0);
    drop(other);
    drop(reused);
    drop(root);
    drop(fs);
    let fs = EasyFileSystem::open(device);
    let root = EasyFileSystem::root_inode(&fs);
    let reopened = root.find("persisted").unwrap();
    assert_eq!(reopened.stat(), (ino, false, 2));
}
