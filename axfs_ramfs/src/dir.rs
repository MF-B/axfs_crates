use alloc::collections::BTreeMap;
use alloc::sync::{Arc, Weak};
use alloc::{string::String, vec::Vec};

use axfs_vfs::{VfsDirEntry, VfsNodeAttr, VfsNodeOps, VfsNodeRef, VfsNodeType};
use axfs_vfs::{VfsError, VfsResult};
use spin::RwLock;

use crate::file::FileNode;
use crate::symlink::SymlinkNode;

/// The directory node in the RAM filesystem.
///
/// It implements [`axfs_vfs::VfsNodeOps`].
pub struct DirNode {
    this: Weak<DirNode>,
    parent: RwLock<Weak<dyn VfsNodeOps>>,
    children: RwLock<BTreeMap<String, VfsNodeRef>>,
}

impl DirNode {
    pub(super) fn new(parent: Option<Weak<dyn VfsNodeOps>>) -> Arc<Self> {
        Arc::new_cyclic(|this| Self {
            this: this.clone(),
            parent: RwLock::new(parent.unwrap_or_else(|| Weak::<Self>::new())),
            children: RwLock::new(BTreeMap::new()),
        })
    }

    pub(super) fn set_parent(&self, parent: Option<&VfsNodeRef>) {
        *self.parent.write() = parent.map_or(Weak::<Self>::new() as _, Arc::downgrade);
    }

    /// Returns a string list of all entries in this directory.
    pub fn get_entries(&self) -> Vec<String> {
        self.children.read().keys().cloned().collect()
    }

    /// Checks whether a node with the given name exists in this directory.
    pub fn exist(&self, name: &str) -> bool {
        self.children.read().contains_key(name)
    }

    /// Adds an existing node with the given name to this directory.
    pub fn add_node(&self, name: &str, node: VfsNodeRef) -> VfsResult {
        use alloc::collections::btree_map::Entry;
        match self.children.write().entry(name.into()) {
            Entry::Vacant(entry) => {
                entry.insert(node);
                Ok(())
            }
            Entry::Occupied(_) => Err(VfsError::AlreadyExists),
        }
    }

    /// Creates a new node with the given name and type in this directory.
    pub fn create_node(&self, name: &str, ty: VfsNodeType) -> VfsResult {
        if self.exist(name) {
            log::error!("AlreadyExists {name}");
            return Err(VfsError::AlreadyExists);
        }
        let node: VfsNodeRef = match ty {
            VfsNodeType::File => Arc::new(FileNode::new()),
            VfsNodeType::Dir => Self::new(Some(self.this.clone())),
            VfsNodeType::SymLink => {
                // Symlinks should be created through symlink() method, not create_node()
                return Err(VfsError::InvalidInput);
            }
            _ => return Err(VfsError::Unsupported),
        };
        self.children.write().insert(name.into(), node);
        Ok(())
    }

    /// Creates a symlink node in this directory.
    fn create_symlink_node(&self, name: &str, target: &str) -> VfsResult {
        let symlink_node = Arc::new(SymlinkNode::new_static(target.to_string()));
        self.add_node(name, symlink_node)
    }

    /// Removes a node by the given name in this directory.
    pub fn remove_node(&self, name: &str) -> VfsResult {
        let mut children = self.children.write();
        let node = children.get(name).ok_or(VfsError::NotFound)?;
        if let Some(dir) = node.as_any().downcast_ref::<DirNode>() {
            if !dir.children.read().is_empty() {
                return Err(VfsError::DirectoryNotEmpty);
            }
        }
        children.remove(name);
        Ok(())
    }

    /// Helper method to traverse path components (., .., or child names)
    fn traverse_path(&self, name: &str) -> VfsResult<VfsNodeRef> {
        match name {
            "" | "." => Ok(self.this.upgrade().ok_or(VfsError::NotFound)? as VfsNodeRef),
            ".." => self.parent().ok_or(VfsError::NotFound),
            _ => self.children
                .read()
                .get(name)
                .ok_or(VfsError::NotFound)
                .cloned(),
        }
    }
}

impl VfsNodeOps for DirNode {
    fn get_attr(&self) -> VfsResult<VfsNodeAttr> {
        Ok(VfsNodeAttr::new_dir(4096, 0))
    }

    fn parent(&self) -> Option<VfsNodeRef> {
        self.parent.read().upgrade()
    }

    fn lookup(self: Arc<Self>, path: &str) -> VfsResult<VfsNodeRef> {
        let (name, rest) = split_path(path);
        let node = self.traverse_path(name)?;

        if let Some(rest) = rest {
            node.lookup(rest)
        } else {
            Ok(node)
        }
    }

    fn read_dir(&self, start_idx: usize, dirents: &mut [VfsDirEntry]) -> VfsResult<usize> {
        let children = self.children.read();
        let mut children = children.iter().skip(start_idx.max(2) - 2);
        for (i, ent) in dirents.iter_mut().enumerate() {
            match i + start_idx {
                0 => *ent = VfsDirEntry::new(".", VfsNodeType::Dir),
                1 => *ent = VfsDirEntry::new("..", VfsNodeType::Dir),
                _ => {
                    if let Some((name, node)) = children.next() {
                        *ent = VfsDirEntry::new(name, node.get_attr().unwrap().file_type());
                    } else {
                        return Ok(i);
                    }
                }
            }
        }
        Ok(dirents.len())
    }

    fn create(&self, path: &str, ty: VfsNodeType) -> VfsResult {
        let (name, rest) = split_path(path);
        if let Some(rest) = rest {
            self.traverse_path(name)?.create(rest, ty)
        } else if name.is_empty() || name == "." || name == ".." {
            Ok(()) // already exists
        } else {
            self.create_node(name, ty)
        }
    }

    fn remove(&self, path: &str) -> VfsResult {
        let (name, rest) = split_path(path);
        if let Some(rest) = rest {
            self.traverse_path(name)?.remove(rest)
        } else if name.is_empty() || name == "." || name == ".." {
            Err(VfsError::InvalidInput) // cannot remove '.' or '..'
        } else {
            self.remove_node(name)
        }
    }

    fn symlink(&self, target: &str, path: &str) -> VfsResult {
        if target.is_empty() {
            return Err(VfsError::InvalidInput);
        }
        
        let (name, rest) = split_path(path);
        if let Some(rest) = rest {
            self.traverse_path(name)?.symlink(target, rest)
        } else if name.is_empty() || name == "." || name == ".." {
            Err(VfsError::InvalidInput)
        } else {
            self.create_symlink_node(name, target)
        }
    }

    fn readlink(&self, path: &str, buf: &mut [u8]) -> VfsResult<usize> {
        let (name, rest) = split_path(path);
        if let Some(rest) = rest {
            self.traverse_path(name)?.readlink(rest, buf)
        } else if name.is_empty() || name == "." || name == ".." {
            Err(VfsError::InvalidInput)
        } else {
            self.traverse_path(name)?.readlink("", buf)
        }
    }

    axfs_vfs::impl_vfs_dir_default! {}
}

fn split_path(path: &str) -> (&str, Option<&str>) {
    let trimmed_path = path.trim_start_matches('/');
    trimmed_path.find('/').map_or((trimmed_path, None), |n| {
        (&trimmed_path[..n], Some(&trimmed_path[n + 1..]))
    })
}
