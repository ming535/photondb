mod file_builder;
pub(crate) use file_builder::FileBuilder;

mod file_reader;
pub(crate) use file_reader::PageFileReader;

mod info_builder;
pub(crate) use info_builder::FileInfoBuilder;

mod types;
pub(crate) use facade::PageFiles;
pub(crate) use types::{FileInfo, FileMeta};

pub(crate) mod facade {
    use std::{os::unix::prelude::OpenOptionsExt, path::PathBuf};

    use photonio::fs::{File, OpenOptions};

    use super::{file_builder::logical_block_size, file_reader::MetaReader, *};
    use crate::page_store::Result;

    /// The facade for page_file module.
    /// it hides the detail about disk location for caller(after it be created).
    pub(crate) struct PageFiles {
        base: PathBuf,

        file_prefix: String,
        use_direct: bool,
    }

    impl PageFiles {
        /// Create page file facade.
        /// It should be a singleton in the page_store.
        pub(crate) fn new(base: impl Into<PathBuf>, file_prefile: &str) -> Self {
            Self {
                base: base.into(),
                file_prefix: file_prefile.into(),
                use_direct: true,
            }
        }

        /// Create file_builder to write a new page_file.
        pub(crate) async fn new_file_builder(&self, file_id: u32) -> Result<FileBuilder> {
            // TODO: switch to env in suitable time.
            let path = self.base.join(format!("{}_{file_id}", self.file_prefix));
            let flags = self.direct_flags();
            let writer = OpenOptions::new()
                .write(true)
                .custom_flags(flags)
                .create(true)
                .truncate(true)
                .open(path)
                .await
                .expect("open file_id: {file_id}'s file fail");
            let metadata = writer.metadata().await.expect("open file metata fail");
            let block_size = logical_block_size(&metadata).await;
            Ok(FileBuilder::new(
                file_id,
                writer,
                self.use_direct,
                block_size,
            ))
        }

        #[inline]
        fn direct_flags(&self) -> i32 {
            const O_DIRECT_LINUX: i32 = 0x4000;
            const O_DIRECT_AARCH64: i32 = 0x10000;
            if !self.use_direct {
                return 0;
            }
            if cfg!(not(target_os = "linux")) {
                0
            } else if cfg!(target_arch = "aarch64") {
                O_DIRECT_AARCH64
            } else {
                O_DIRECT_LINUX
            }
        }

        /// Open page_reader for a page_file.
        /// page_store could get file_id & block_size from page_addr's high bit
        /// and version.active_files.
        pub(crate) async fn open_page_reader(
            &self,
            file_id: u32,
            block_size: usize,
        ) -> Result<PageFileReader<File>> {
            let path = self.base.join(format!("{}_{}", self.file_prefix, file_id));
            let flags = self.direct_flags();
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(flags)
                .open(path)
                .await
                .expect("open reader for file_id: {file_id} fail");
            Ok(PageFileReader::from(file, self.use_direct, block_size))
        }

        // Create info_builder to help recovery & mantains version's file_info.
        pub(crate) fn new_info_builder(&self) -> FileInfoBuilder {
            FileInfoBuilder::new(self.base.to_owned(), &self.file_prefix)
        }

        pub(crate) async fn open_meta_reader(&self, file_id: u32) -> Result<MetaReader<File>> {
            let path = self.base.join(format!("{}_{}", self.file_prefix, file_id));
            let file = File::open(path)
                .await
                .expect("open reader for file_id: {file_id} fail");
            let raw_metadata = file.metadata().await.expect("read fs metadata fail");
            let block_size = logical_block_size(&raw_metadata).await;
            let page_file_reader = PageFileReader::from(file, true, block_size);
            MetaReader::open(page_file_reader, raw_metadata.len() as u32, file_id).await
        }

        pub(crate) async fn remove_files(&self, files: Vec<u32>) -> Result<()> {
            for file_id in files {
                // FIXME: handle error.
                self.remove_file(file_id).await?;
            }
            Ok(())
        }

        async fn remove_file(&self, file_id: u32) -> Result<()> {
            let path = self.base.join(format!("{}_{}", self.file_prefix, file_id));
            photonio::fs::remove_file(&path)
                .await
                .expect("remove file failed");
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use std::collections::HashMap;

        use super::*;

        #[photonio::test]
        fn test_file_builder() {
            let base = std::env::temp_dir();
            let files = PageFiles::new(&base, "test_builder");
            let mut builder = files.new_file_builder(11233).await.unwrap();
            builder.add_delete_pages(&[1, 2]);
            builder.add_page(3, 1, &[3, 4, 1]).await.unwrap();
            builder.finish().await.unwrap();
        }

        #[photonio::test]
        fn test_read_page() {
            let files = {
                let base = std::env::temp_dir();
                PageFiles::new(&base, "test_dread")
            };
            let file_id = 2;
            let info = {
                let mut b = files.new_file_builder(file_id).await.unwrap();
                b.add_delete_pages(&[page_addr(1, 0), page_addr(1, 1)]);
                b.add_page(1, page_addr(2, 2), &[7].repeat(8192))
                    .await
                    .unwrap();
                b.add_page(2, page_addr(2, 3), &[8].repeat(8192 / 2))
                    .await
                    .unwrap();
                b.add_page(3, page_addr(2, 4), &[9].repeat(8192 / 3))
                    .await
                    .unwrap();
                let info = b.finish().await.unwrap();
                assert_eq!(info.effective_size(), 8192 + 8192 / 2 + 8192 / 3);
                info
            };

            let page_reader = files
                .open_page_reader(info.meta().get_file_id(), 4096)
                .await
                .unwrap();

            {
                // read aligned 1st page.
                let hd = info.get_page_handle(page_addr(2, 2)).unwrap();
                let mut buf = vec![0u8; hd.size as usize];
                page_reader
                    .read_exact_at(&mut buf, hd.offset as u64)
                    .await
                    .unwrap();
            }

            {
                // read unaligned(need trim end) 2nd page.
                let hd = info.get_page_handle(page_addr(2, 3)).unwrap();
                let mut buf = vec![0u8; hd.size as usize];
                page_reader
                    .read_exact_at(&mut buf, hd.offset as u64)
                    .await
                    .unwrap();
            }

            {
                // read unaligned(need trim both start and end) 3rd page.
                let hd = info.get_page_handle(page_addr(2, 3)).unwrap();
                let mut buf = vec![0u8; hd.size as usize];
                page_reader
                    .read_exact_at(&mut buf, hd.offset as u64)
                    .await
                    .unwrap();
            }
        }

        #[photonio::test]
        fn test_test_simple_write_reader() {
            let files = {
                let base = std::env::temp_dir();
                PageFiles::new(&base, "test_simple_rw")
            };

            let file_id = 2;
            let ret_info = {
                let mut b = files.new_file_builder(file_id).await.unwrap();
                b.add_delete_pages(&[page_addr(1, 0), page_addr(1, 1)]);
                b.add_page(1, page_addr(2, 2), &[7].repeat(8192))
                    .await
                    .unwrap();
                b.add_page(2, page_addr(2, 3), &[8].repeat(8192 / 2))
                    .await
                    .unwrap();
                b.add_page(3, page_addr(2, 4), &[9].repeat(8192 / 3))
                    .await
                    .unwrap();
                let info = b.finish().await.unwrap();
                assert_eq!(info.effective_size(), 8192 + 8192 / 2 + 8192 / 3);
                info
            };
            {
                let meta = {
                    let meta_reader = files.open_meta_reader(file_id).await.unwrap();
                    assert_eq!(
                        meta_reader.file_metadata().total_page_size(),
                        8192 + 8192 / 2 + 8192 / 3
                    );
                    let page3 = page_addr(2, 4);
                    let (page3_offset, page3_size) =
                        meta_reader.file_metadata().get_page_handle(page3).unwrap();
                    let handle = ret_info.get_page_handle(page3).unwrap();
                    assert_eq!(page3_offset as u32, handle.offset);
                    assert_eq!(page3_size, 8192 / 3);

                    let page_table = meta_reader.read_page_table().await.unwrap();
                    assert_eq!(page_table.len(), 3);

                    let delete_tables = meta_reader.read_delete_pages().await.unwrap();
                    assert_eq!(delete_tables.len(), 2);
                    meta_reader.file_metadata()
                };

                {
                    let (page3_offset, page3_size) = meta.get_page_handle(page_addr(2, 4)).unwrap();
                    let page_reader = files.open_page_reader(file_id, 4096).await.unwrap();
                    let mut buf = vec![0u8; page3_size];
                    page_reader
                        .read_exact_at(&mut buf, page3_offset)
                        .await
                        .unwrap();
                    assert_eq!(buf.as_slice(), &[9].repeat(buf.len()));
                }
            }
        }

        #[photonio::test]
        fn test_file_info_recovery_and_add_new_file() {
            let files = {
                let base = std::env::temp_dir();
                PageFiles::new(&base, "test_recovery")
            };

            let info_builder = files.new_info_builder();
            {
                // test add new files.
                let mut mock_version = HashMap::new();
                {
                    let file_id = 1;
                    let mut b = files.new_file_builder(file_id).await.unwrap();
                    b.add_page(1, page_addr(file_id, 0), &[1].repeat(10))
                        .await
                        .unwrap();
                    b.add_page(2, page_addr(file_id, 1), &[2].repeat(10))
                        .await
                        .unwrap();
                    b.add_page(3, page_addr(file_id, 2), &[3].repeat(10))
                        .await
                        .unwrap();
                    let file_info = b.finish().await.unwrap();
                    mock_version.insert(file_id, file_info);
                }

                {
                    // add an additional file with delete file1's page info.
                    let file_id = 2;
                    let delete_pages = &[page_addr(1, 0)];

                    let mut b = files.new_file_builder(file_id).await.unwrap();
                    b.add_page(4, page_addr(file_id, 0), &[1].repeat(10))
                        .await
                        .unwrap();
                    b.add_page(5, page_addr(file_id, 1), &[2].repeat(10))
                        .await
                        .unwrap();
                    b.add_page(6, page_addr(file_id, 4), &[3].repeat(10))
                        .await
                        .unwrap();
                    b.add_delete_pages(delete_pages);
                    let file_info = b.finish().await.unwrap();

                    let original_file1_active_size = mock_version.get(&1).unwrap().effective_size();
                    mock_version = info_builder
                        .add_file_info(&mock_version, file_info, delete_pages)
                        .unwrap();

                    let file1 = mock_version.get(&1).unwrap();
                    assert_eq!(file1.effective_size(), original_file1_active_size - 10);
                    assert!(file1.get_page_handle(page_addr(1, 0)).is_none());
                    assert!(file1.get_page_handle(page_addr(1, 1)).is_some());

                    let file2 = mock_version.get(&2).unwrap();
                    let hd = file2.get_page_handle(page_addr(2, 4)).unwrap();
                    assert_eq!(hd.size, 10);
                    assert_eq!(hd.offset, 20);
                }
            }

            {
                // test recovery file_info from folder.
                let known_files = &[1, 2].iter().cloned().map(Into::into).collect::<Vec<_>>();
                let recovery_mock_version = info_builder
                    .recovery_base_file_infos(known_files)
                    .await
                    .unwrap();
                let file1 = recovery_mock_version.get(&1).unwrap();
                assert_eq!(file1.effective_size(), 20);
                assert!(file1.get_page_handle(page_addr(1, 0)).is_none());
                let file2 = recovery_mock_version.get(&2).unwrap();
                assert_eq!(file2.effective_size(), 30);
                let file2 = recovery_mock_version.get(&2).unwrap();
                let hd = file2.get_page_handle(page_addr(2, 4)).unwrap();
                assert_eq!(hd.size, 10);
                assert_eq!(hd.offset, 20);
            }
        }

        fn page_addr(file_id: u32, index: u32) -> u64 {
            ((file_id as u64) << 32) | (index as u64)
        }
    }
}
