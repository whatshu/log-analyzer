use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

use rayon::prelude::*;

use crate::error::{LogAnalyzerError, Result};

/// Compressed chunk storage using zstd.
pub struct ChunkStorage {
    chunks_dir: PathBuf,
}

impl ChunkStorage {
    pub fn new(chunks_dir: PathBuf) -> Self {
        Self { chunks_dir }
    }

    /// Write multiple chunks in parallel with zstd compression.
    pub fn write_chunks(&self, chunks: &[Vec<u8>]) -> Result<()> {
        fs::create_dir_all(&self.chunks_dir)?;

        chunks
            .par_iter()
            .enumerate()
            .try_for_each(|(id, data)| -> Result<()> {
                self.write_chunk(id as u32, data)
            })?;

        Ok(())
    }

    /// Write a single compressed chunk.
    pub fn write_chunk(&self, id: u32, data: &[u8]) -> Result<()> {
        let path = self.chunk_path(id);
        let compressed = zstd::encode_all(data, 3).map_err(|e| {
            LogAnalyzerError::Compression(format!("Failed to compress chunk {}: {}", id, e))
        })?;

        let mut file = fs::File::create(path)?;
        file.write_all(&compressed)?;
        Ok(())
    }

    /// Read and decompress a chunk.
    pub fn read_chunk(&self, id: u32) -> Result<Vec<u8>> {
        let path = self.chunk_path(id);
        let compressed = fs::read(&path)?;

        let mut decoder = zstd::Decoder::new(compressed.as_slice()).map_err(|e| {
            LogAnalyzerError::Compression(format!("Failed to decompress chunk {}: {}", id, e))
        })?;

        let mut data = Vec::new();
        decoder.read_to_end(&mut data).map_err(|e| {
            LogAnalyzerError::Compression(format!("Failed to read chunk {}: {}", id, e))
        })?;

        Ok(data)
    }

    /// Get the total number of chunks on disk.
    pub fn chunk_count(&self) -> Result<usize> {
        let mut count = 0;
        if self.chunks_dir.exists() {
            for entry in fs::read_dir(&self.chunks_dir)? {
                let entry = entry?;
                if entry.path().extension().is_some_and(|e| e == "zst") {
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    fn chunk_path(&self, id: u32) -> PathBuf {
        self.chunks_dir.join(format!("{:06}.zst", id))
    }
}
