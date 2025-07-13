use alloc::{boxed::Box, string::String};
use axfs_vfs::{VfsNodeAttr, VfsNodeOps, VfsNodeType, VfsResult};

/// Callback function type for dynamic symlink target generation.
pub type SymlinkCallback = Box<dyn Fn() -> String + Send + Sync>;

/// Generator for symlink targets supporting both static and dynamic variants.
pub enum SymlinkGenerator {
    /// Static symlink with a fixed target.
    Static(String),
    /// Dynamic symlink with runtime callback.
    Dynamic(SymlinkCallback),
}

/// Unified symbolic link node that supports both static and dynamic targets.
pub struct SymlinkNode {
    generator: SymlinkGenerator,
}

impl SymlinkNode {
    /// Create a static symlink with a fixed target.
    pub fn new_static(target: String) -> Self {
        Self {
            generator: SymlinkGenerator::Static(target),
        }
    }

    /// Create a dynamic symlink with a runtime callback.
    pub fn new_dynamic<F>(callback: F) -> Self
    where
        F: Fn() -> String + Send + Sync + 'static,
    {
        Self {
            generator: SymlinkGenerator::Dynamic(Box::new(callback)),
        }
    }

    /// Get the current target of this symlink.
    pub fn target(&self) -> String {
        match &self.generator {
            SymlinkGenerator::Static(target) => target.to_owned(),
            SymlinkGenerator::Dynamic(callback) => callback(),
        }
    }

}

impl VfsNodeOps for SymlinkNode {
    fn get_attr(&self) -> VfsResult<VfsNodeAttr> {
        let target = self.target();
        Ok(VfsNodeAttr::new(
            axfs_vfs::VfsNodePerm::default_file(),
            VfsNodeType::SymLink,
            target.len() as u64,
            0,
        ))
    }

    fn readlink(&self, _path: &str, buf: &mut [u8]) -> VfsResult<usize> {
        let target = self.target();
        let target_bytes = target.as_bytes();
        let copy_len = buf.len().min(target_bytes.len());
        buf[..copy_len].copy_from_slice(&target_bytes[..copy_len]);
        Ok(copy_len)
    }

    fn is_symlink(&self) -> bool {
        true
    }

    axfs_vfs::impl_vfs_non_dir_default! {}
}