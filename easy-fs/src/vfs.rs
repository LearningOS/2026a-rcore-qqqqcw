use super::{
    block_cache_sync_all, get_block_cache, BlockDevice, DirEntry, DiskInode, DiskInodeType,
    EasyFileSystem, DIRENT_SZ,
};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::{Mutex, MutexGuard};
/// Virtual filesystem layer over easy-fs
pub struct Inode {
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

impl Inode {
    /// Create a vfs inode
    pub fn new(
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
    ) -> Self {
        Self {
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
        }
    }
    /// Call a function over a disk inode to read it
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .read(self.block_offset, f)
    }
    /// Call a function over a disk inode to modify it
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .modify(self.block_offset, f)
    }
    /// Find inode under a disk inode by name
    fn find_inode_id(&self, name: &str, disk_inode: &DiskInode) -> Option<u32> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device,),
                DIRENT_SZ,
            );
            if dirent.name() == name {
                return Some(dirent.inode_id() as u32);
            }
        }
        None
    }
    /// Return the shared in-memory handle for an inode.
    fn cached_inode(&self, id: u32, fs: &mut EasyFileSystem) -> Arc<Inode> {
        if let Some(inode) = fs.inode_cache.get(&id).and_then(|weak| weak.upgrade()) {
            return inode;
        }
        let (block_id, offset) = fs.get_disk_inode_pos(id);
        let inode = Arc::new(Self::new(
            block_id,
            offset,
            self.fs.clone(),
            self.block_device.clone(),
        ));
        fs.inode_cache.insert(id, Arc::downgrade(&inode));
        inode
    }
    /// Find inode under current inode by name.
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        let mut fs = self.fs.lock();
        let id = self.read_disk_inode(|disk| self.find_inode_id(name, disk))?;
        Some(self.cached_inode(id, &mut fs))
    }
    /// Increase the size of a disk inode
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        if new_size < disk_inode.size {
            return;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            v.push(fs.alloc_data());
        }
        disk_inode.increase_size(new_size, v, &self.block_device);
    }
    /// Create inode under current inode by name
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        if !Self::valid_name(name) {
            return None;
        }
        let mut fs = self.fs.lock();
        let op = |root_inode: &DiskInode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        };
        if self.read_disk_inode(op).is_some() {
            return None;
        }
        // create a new file
        // alloc a inode with an indirect block
        let new_inode_id = fs.alloc_inode();
        // initialize inode
        let (new_inode_block_id, new_inode_block_offset) = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(new_inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
                new_inode.initialize(DiskInodeType::File);
            });
        self.modify_disk_inode(|root_inode| {
            // Reuse a vacant slot before extending the directory.
            let count = root_inode.size as usize / DIRENT_SZ;
            let mut empty = DirEntry::empty();
            let index = (0..count)
                .find(|i| {
                    root_inode.read_at(i * DIRENT_SZ, empty.as_bytes_mut(), &self.block_device);
                    empty.name().is_empty()
                })
                .unwrap_or(count);
            if index == count {
                self.increase_size(((count + 1) * DIRENT_SZ) as u32, root_inode, &mut fs);
            }
            root_inode.write_at(
                index * DIRENT_SZ,
                DirEntry::new(name, new_inode_id).as_bytes(),
                &self.block_device,
            );
        });

        block_cache_sync_all();
        Some(self.cached_inode(new_inode_id, &mut fs))
    }

    fn valid_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 27
            && !name.contains('/')
            && !name.as_bytes().contains(&0)
            && name != "."
            && name != ".."
    }

    /// Add another directory entry referring to the same regular file.
    pub fn link(&self, old: &str, new: &str) -> isize {
        if !Self::valid_name(old) || !Self::valid_name(new) {
            return -1;
        }
        let mut fs = self.fs.lock();
        let id = self.read_disk_inode(|disk| {
            if self.find_inode_id(new, disk).is_some() {
                None
            } else {
                self.find_inode_id(old, disk)
            }
        });
        let id = match id {
            Some(id) => id,
            None => return -1,
        };
        let (block, offset) = fs.get_disk_inode_pos(id);
        if !get_block_cache(block as usize, self.block_device.clone())
            .lock()
            .read(offset, |disk: &DiskInode| disk.is_file())
        {
            return -1;
        }
        self.modify_disk_inode(|root| {
            let count = root.size as usize / DIRENT_SZ;
            let mut entry = DirEntry::empty();
            let index = (0..count)
                .find(|i| {
                    root.read_at(i * DIRENT_SZ, entry.as_bytes_mut(), &self.block_device);
                    entry.name().is_empty()
                })
                .unwrap_or(count);
            if index == count {
                self.increase_size(((count + 1) * DIRENT_SZ) as u32, root, &mut fs);
            }
            root.write_at(
                index * DIRENT_SZ,
                DirEntry::new(new, id).as_bytes(),
                &self.block_device,
            );
        });
        block_cache_sync_all();
        0
    }

    /// Remove a name; an open handle keeps the unlinked inode alive.
    pub fn unlink(&self, name: &str) -> isize {
        if !Self::valid_name(name) {
            return -1;
        }
        let mut fs = self.fs.lock();
        let found = self.read_disk_inode(|root| {
            let mut entry = DirEntry::empty();
            for i in 0..root.size as usize / DIRENT_SZ {
                root.read_at(i * DIRENT_SZ, entry.as_bytes_mut(), &self.block_device);
                if entry.name() == name {
                    return Some((i, entry.inode_id() as u32));
                }
            }
            None
        });
        let (index, id) = match found {
            Some(found) => found,
            None => return -1,
        };
        self.modify_disk_inode(|root| {
            root.write_at(
                index * DIRENT_SZ,
                DirEntry::empty().as_bytes(),
                &self.block_device,
            );
        });
        let open = fs
            .inode_cache
            .get(&id)
            .map_or(false, |weak| weak.strong_count() != 0);
        if !open && Self::count_links(&fs, &self.block_device, id) == 0 {
            Self::reclaim(&mut fs, &self.block_device, id);
        }
        block_cache_sync_all();
        0
    }

    fn count_links(fs: &EasyFileSystem, device: &Arc<dyn BlockDevice>, id: u32) -> u32 {
        let (block, offset) = fs.get_disk_inode_pos(0);
        get_block_cache(block as usize, device.clone())
            .lock()
            .read(offset, |root: &DiskInode| {
                let mut count = 0;
                let mut entry = DirEntry::empty();
                for i in 0..root.size as usize / DIRENT_SZ {
                    root.read_at(i * DIRENT_SZ, entry.as_bytes_mut(), device);
                    if entry.inode_id() == id {
                        count += 1;
                    }
                }
                count
            })
    }

    fn reclaim(fs: &mut EasyFileSystem, device: &Arc<dyn BlockDevice>, id: u32) {
        let (block, offset) = fs.get_disk_inode_pos(id);
        let data = get_block_cache(block as usize, device.clone())
            .lock()
            .modify(offset, |disk: &mut DiskInode| disk.clear_size(device));
        for block in data {
            fs.dealloc_data(block);
        }
        fs.dealloc_inode(id);
        fs.inode_cache.remove(&id);
    }

    /// Return inode identity, file kind, and persistent namespace link count.
    pub fn stat(&self) -> (u64, bool, u32) {
        let fs = self.fs.lock();
        let id = fs.get_disk_inode_id(self.block_id, self.block_offset);
        let is_dir = self.read_disk_inode(|disk| disk.is_dir());
        (
            id as u64,
            is_dir,
            Self::count_links(&fs, &self.block_device, id),
        )
    }

    /// List inodes under current inode
    pub fn ls(&self) -> Vec<String> {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                if !dirent.name().is_empty() {
                    v.push(String::from(dirent.name()));
                }
            }
            v
        })
    }
    /// Read data from current inode
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| disk_inode.read_at(offset, buf, &self.block_device))
    }
    /// Write data to current inode
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        let mut fs = self.fs.lock();
        let size = self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        block_cache_sync_all();
        size
    }
    /// Clear the data in current inode
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            let size = disk_inode.size;
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
        block_cache_sync_all();
    }
}

impl Drop for Inode {
    fn drop(&mut self) {
        let mut fs = self.fs.lock();
        let id = fs.get_disk_inode_id(self.block_id, self.block_offset);
        if id != 0 {
            fs.inode_cache.remove(&id);
            if Self::count_links(&fs, &self.block_device, id) == 0 {
                Self::reclaim(&mut fs, &self.block_device, id);
                block_cache_sync_all();
            }
        }
    }
}
