//! Resource detection for optimal parallel test execution

use std::process::Command;

#[derive(Debug, Clone)]
pub struct SystemResources {
    pub available_cpus: usize,
    pub total_memory_mb: usize,
    pub available_memory_mb: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ResourceError {
    #[error("Failed to detect system resources: {0}")]
    DetectionError(String),
}

impl SystemResources {
    /// Detect available system resources
    pub fn detect() -> Result<Self, ResourceError> {
        let available_cpus = detect_available_cpus()?;
        let (total_memory_mb, available_memory_mb) = detect_memory()?;
        
        Ok(Self {
            available_cpus,
            total_memory_mb,
            available_memory_mb,
        })
    }
    
    /// Calculate optimal parallel test limit
    pub fn calculate_parallel_limit(&self, memory_per_test_mb: usize) -> usize {
        // Base limit on CPU cores
        let cpu_limit = self.available_cpus;
        
        // Calculate memory-based limit
        let memory_limit = if memory_per_test_mb > 0 {
            self.available_memory_mb / memory_per_test_mb
        } else {
            usize::MAX
        };
        
        // Use the more restrictive limit
        std::cmp::min(cpu_limit, memory_limit)
    }
    
    /// Safe parallel limit with minimum guarantee
    pub fn safe_parallel_limit(&self, memory_per_test_mb: usize) -> usize {
        let limit = self.calculate_parallel_limit(memory_per_test_mb);
        
        // Ensure at least 1 test can run
        std::cmp::max(1, limit)
    }
}

/// Detect available CPU cores
fn detect_available_cpus() -> Result<usize, ResourceError> {
    // First try to get available CPUs (considering cgroups limits)
    if let Ok(available) = std::thread::available_parallelism() {
        return Ok(available.get());
    }
    
    // Fallback to total CPUs
    let output = Command::new("nproc")
        .output()
        .map_err(|e| ResourceError::DetectionError(e.to_string()))?;
    
    if output.status.success() {
        let cpus_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        cpus_str.parse::<usize>()
            .map_err(|e| ResourceError::DetectionError(e.to_string()))
    } else {
        // Ultimate fallback
        Ok(num_cpus::get())
    }
}

/// Detect system memory
fn detect_memory() -> Result<(usize, usize), ResourceError> {
    #[cfg(target_os = "linux")]
    {
        use std::fs;
        
        // Read /proc/meminfo for Linux systems
        let meminfo = fs::read_to_string("/proc/meminfo")
            .map_err(|e| ResourceError::DetectionError(e.to_string()))?;
        
        let mut total_memory_kb = None;
        let mut available_memory_kb = None;
        
        for line in meminfo.lines() {
            if let Some((key, value)) = line.split_once(':') {
                let value = value.trim().split_whitespace().next().unwrap_or("0");
                
                match key.trim() {
                    "MemTotal" => {
                        total_memory_kb = value.parse::<usize>().ok();
                    }
                    "MemAvailable" => {
                        available_memory_kb = value.parse::<usize>().ok();
                    }
                    _ => {}
                }
            }
        }
        
        if let (Some(total), Some(available)) = (total_memory_kb, available_memory_kb) {
            let total_mb = total / 1024;
            let available_mb = available / 1024;
            return Ok((total_mb, available_mb));
        }
    }
    
    // Fallback for non-Linux systems or if /proc/meminfo fails
    let output = Command::new("free")
        .arg("-m")
        .output()
        .map_err(|e| ResourceError::DetectionError(e.to_string()))?;
    
    if output.status.success() {
        let free_output = String::from_utf8_lossy(&output.stdout);
        parse_free_output(&free_output)
    } else {
        // Conservative fallback
        Ok((4096, 2048)) // 4GB total, 2GB available
    }
}

/// Parse output of `free -m` command
fn parse_free_output(output: &str) -> Result<(usize, usize), ResourceError> {
    for line in output.lines() {
        if line.starts_with("Mem:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 7 {
                let total = parts[1].parse::<usize>()
                    .map_err(|e| ResourceError::DetectionError(e.to_string()))?;
                let available = parts[6].parse::<usize>()
                    .map_err(|e| ResourceError::DetectionError(e.to_string()))?;
                return Ok((total, available));
            }
        }
    }
    
    Err(ResourceError::DetectionError(
        "Failed to parse 'free' command output".to_string()
    ))
}