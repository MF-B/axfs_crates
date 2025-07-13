//! RAM filesystem used by [ArceOS](https://github.com/arceos-org/arceos).
//!
//! The implementation is based on [`axfs_vfs`].

#![cfg_attr(not(test), no_std)]

extern crate alloc;

mod dir;
mod file;
mod symlink;

#[cfg(test)]
mod tests;

pub use self::dir::DirNode;
pub use self::file::FileNode;
pub use self::symlink::SymlinkNode;

use alloc::sync::Arc;
use axfs_vfs::{VfsNodeOps, VfsNodeRef, VfsOps, VfsResult};
use spin::once::Once;

/// A RAM filesystem that implements [`axfs_vfs::VfsOps`].
pub struct RamFileSystem {
    parent: Once<VfsNodeRef>,
    root: Arc<DirNode>,
}

impl RamFileSystem {
    /// Create a new instance.
    pub fn new() -> Self {
        Self {
            parent: Once::new(),
            root: DirNode::new(None),
        }
    }

    /// Returns the root directory node in [`Arc<DirNode>`](DirNode).
    pub fn root_dir_node(&self) -> Arc<DirNode> {
        self.root.clone()
    }

    /// Add a dynamic symlink to the filesystem.
    /// This is a convenience method for creating symlinks with runtime-generated targets.
    pub fn add_dynamic_symlink<F>(&self, path: &str, generator: F) -> VfsResult
    where
        F: Fn() -> alloc::string::String + Send + Sync + 'static,
    {
        let symlink = Arc::new(SymlinkNode::new_dynamic(generator));

        // Parse path to get parent directory and name
        if let Some((parent_path, name)) = path.rsplit_once('/') {
            let parent_dir = if parent_path.is_empty() {
                self.root.clone()
            } else {
                self.root.clone().lookup(parent_path)?
            };

            if let Some(dir) = parent_dir.as_any().downcast_ref::<DirNode>() {
                dir.add_node(name, symlink)
            } else {
                Err(axfs_vfs::VfsError::NotADirectory)
            }
        } else {
            // No parent path, add to root
            self.root.add_node(path, symlink)
        }
    }
}

impl VfsOps for RamFileSystem {
    fn mount(&self, _path: &str, mount_point: VfsNodeRef) -> VfsResult {
        if let Some(parent) = mount_point.parent() {
            self.root.set_parent(Some(self.parent.call_once(|| parent)));
        } else {
            self.root.set_parent(None);
        }
        Ok(())
    }

    fn root_dir(&self) -> VfsNodeRef {
        self.root.clone()
    }
}

impl Default for RamFileSystem {
    fn default() -> Self {
        Self::new()
    }
}
