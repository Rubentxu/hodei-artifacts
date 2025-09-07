use std::process::Command;
use std::time::Duration;
use tokio::time::sleep;

pub struct ComposeTestEnvironment {
    compose_file_path: String,
}

impl ComposeTestEnvironment {
    pub async fn new(compose_file_path: &str) -> Self {
        println!("Starting Docker Compose environment from: {}", compose_file_path);

        let output = Command::new("docker")
            .arg("compose")
            .arg("-f")
            .arg(compose_file_path)
            .arg("up")
            .arg("-d")
            .output()
            .expect("Failed to execute docker compose up");

        if !output.status.success() {
            eprintln!("Error starting Docker Compose: {}", String::from_utf8_lossy(&output.stderr));
            panic!("Failed to start Docker Compose environment.");
        }
        println!("Docker Compose environment started.");

        // Wait for MongoDB to be ready
        println!("Waiting for MongoDB to be ready...");
        let mut retries = 0;
        let max_retries = 30; // 30 * 1 second = 30 seconds timeout
        loop {
            let health_check_output = Command::new("docker")
                .arg("compose")
                .arg("-f")
                .arg(compose_file_path)
                .arg("exec")
                .arg("mongodb")
                .arg("mongosh")
                .arg("--eval")
                .arg("db.adminCommand('ping').ok")
                .arg("--quiet")
                .output()
                .expect("Failed to execute mongosh health check");

            if health_check_output.status.success() && String::from_utf8_lossy(&health_check_output.stdout).trim() == "1" {
                println!("MongoDB is ready!");
                break;
            }

            retries += 1;
            if retries >= max_retries {
                eprintln!("MongoDB health check failed after {} retries. Output: {}", max_retries, String::from_utf8_lossy(&health_check_output.stderr));
                panic!("MongoDB did not become ready in time.");
            }

            sleep(Duration::from_secs(1)).await;
        }

        ComposeTestEnvironment {
            compose_file_path: compose_file_path.to_string(),
        }
    }
}

impl Drop for ComposeTestEnvironment {
    fn drop(&mut self) {
        println!("Tearing down Docker Compose environment from: {}", self.compose_file_path);
        let output = Command::new("docker")
            .arg("compose")
            .arg("-f")
            .arg(&self.compose_file_path)
            .arg("down")
            .output()
            .expect("Failed to execute docker compose down");

        if !output.status.success() {
            eprintln!("Error tearing down Docker Compose: {}", String::from_utf8_lossy(&output.stderr));
        }
        println!("Docker Compose environment torn down.");
    }
}
