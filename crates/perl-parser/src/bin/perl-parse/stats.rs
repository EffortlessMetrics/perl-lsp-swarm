use std::io::{self, Write};
use std::time::Duration;

#[derive(Default)]
pub(crate) struct TotalStats {
    files_parsed: usize,
    files_failed: usize,
    total_bytes: usize,
    total_time: Duration,
    total_nodes: usize,
    file_details: Vec<FileStats>,
}

struct FileStats {
    name: String,
    bytes: usize,
    time: Duration,
    nodes: usize,
    error: bool,
}

impl TotalStats {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn add_file(&mut self, name: &str, bytes: usize, time: Duration, nodes: usize) {
        self.files_parsed += 1;
        self.total_bytes += bytes;
        self.total_time += time;
        self.total_nodes += nodes;
        self.file_details.push(FileStats {
            name: name.to_string(),
            bytes,
            time,
            nodes,
            error: false,
        });
    }

    pub(crate) fn add_error(&mut self, name: &str) {
        self.files_failed += 1;
        self.file_details.push(FileStats {
            name: name.to_string(),
            bytes: 0,
            time: Duration::ZERO,
            nodes: 0,
            error: true,
        });
    }

    pub(crate) fn write(&self, out: &mut impl Write) -> io::Result<()> {
        writeln!(out, "\n=== Total Statistics ===")?;
        writeln!(out, "Files parsed: {}", self.files_parsed)?;
        writeln!(out, "Files failed: {}", self.files_failed)?;
        writeln!(
            out,
            "Total size: {} bytes ({:.2} KB)",
            self.total_bytes,
            self.total_bytes as f64 / 1024.0
        )?;
        writeln!(out, "Total time: {:?}", self.total_time)?;
        writeln!(out, "Total nodes: {}", self.total_nodes)?;

        if let Some(avg_nodes) = self.total_nodes.checked_div(self.files_parsed) {
            let avg_speed = self.total_bytes as f64 / self.total_time.as_secs_f64() / 1_000_000.0;
            writeln!(out, "Average speed: {avg_speed:.2} MB/s")?;
            writeln!(out, "Average nodes per file: {avg_nodes}")?;
        }

        if self.file_details.len() > 1 && self.file_details.len() <= 20 {
            writeln!(out, "\n=== File Details ===")?;
            for stat in &self.file_details {
                if stat.error {
                    writeln!(out, "{}: FAILED", stat.name)?;
                } else {
                    writeln!(
                        out,
                        "{}: {} bytes, {:?}, {} nodes",
                        stat.name, stat.bytes, stat.time, stat.nodes
                    )?;
                }
            }
        }

        Ok(())
    }
}
