use memmap2::{Mmap, MmapMut};
use std::fs::{File, OpenOptions};
// use std::os::unix::fs::OpenOptionsExt;

use crate::instance::InstanceVersion;
use crate::synchronizer::SynchronizerError;
use crate::synchronizer::SynchronizerError::*;

/// Data container stores memory mapped data files allowing
/// to switch between them when data instance version is changed
pub(crate) struct DataContainer {
    /// Base data path
    base_path: String,
    /// Reader's current local instance version
    version: Option<InstanceVersion>,
    /// Read-only memory mapped files storing data
    idx_mmaps: [Option<Mmap>; 2],
}

impl DataContainer {
    /// Create new instance of `DataContainer`
    pub(crate) fn new(base_path: String) -> Self {
        DataContainer {
            base_path,
            version: None,
            idx_mmaps: [None, None],
        }
    }

    /// Write `data` into mapped data file with given `version`
    pub(crate) fn write(
        &mut self,
        data: &[u8],
        version: InstanceVersion,
    ) -> Result<usize, SynchronizerError> {
        let data_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            // .mode(0o640) // set file mode to allow read/write from owner, read from group only
            .open(version.path(&self.base_path))
            .map_err(FailedDataWrite)?;

        // grow data file when its current length exceeded
        let data_len = data.len() as u64;
        if data_len > data_file.metadata().map_err(FailedDataWrite)?.len() {
            data_file.set_len(data_len).map_err(FailedDataWrite)?;
        }

        // copy data to mapped file and ensure it's been flushed
        let mut mmap = unsafe { MmapMut::map_mut(&data_file).map_err(FailedDataWrite)? };
        mmap[..data.len()].copy_from_slice(data);
        mmap.flush().map_err(FailedDataWrite)?;

        Ok(data.len())
    }

    /// Fetch data from mapped data file of given `version`
    #[inline]
    pub(crate) fn data(
        &mut self,
        version: InstanceVersion,
    ) -> Result<(&[u8], bool), SynchronizerError> {
        let mmap = &mut self.idx_mmaps[version.idx()];
        let data_size = version.size();

        // only open and mmap data file in the following cases:
        // * if it never was opened/mapped before
        // * if current mmap size is smaller than requested data size
        if mmap.is_none() || mmap.as_ref().unwrap().len() < data_size {
            let data_file = File::open(version.path(&self.base_path)).map_err(FailedDataRead)?;
            if data_file.metadata().map_err(FailedDataRead)?.len() < data_size as u64 {
                return Err(FailedEntityRead);
            }
            *mmap = Some(unsafe { Mmap::map(&data_file).map_err(FailedDataRead)? });
        }

        let data = &mmap.as_ref().unwrap()[..data_size];
        let new_version = Some(version);
        let switched = new_version != self.version;
        self.version = new_version;

        Ok((data, switched))
    }
}
