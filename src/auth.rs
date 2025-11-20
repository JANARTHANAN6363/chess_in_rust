use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

const USERS_FILE: &str = "users.txt";

#[derive(Clone, Debug)]
pub struct User {
    pub username: String,
    password_hash: String,
}

impl User {
    pub fn new(username: String, password: String) -> Self {
        Self {
            username,
            password_hash: Self::hash_password(&password),
        }
    }

    fn hash_password(password: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn verify_password(&self, password: &str) -> bool {
        self.password_hash == Self::hash_password(password)
    }
}

pub struct AuthSystem {
    current_user: Option<User>,
}

impl AuthSystem {
    pub fn new() -> Self {
        if !Path::new(USERS_FILE).exists() {
            File::create(USERS_FILE).expect("Failed to create users file");
        }
        Self { current_user: None }
    }

    pub fn register(&mut self) -> io::Result<bool> {
        println!("\n=== REGISTER ===");
        print!("Enter username: ");
        io::stdout().flush()?;
        let mut username = String::new();
        io::stdin().read_line(&mut username)?;
        let username = username.trim().to_string();

        if username.is_empty() {
            println!("❌ Username cannot be empty!");
            return Ok(false);
        }

        if username.contains(':') {
            println!("❌ Username cannot contain ':' character!");
            return Ok(false);
        }

        if self.user_exists(&username)? {
            println!("❌ Username '{}' already exists!", username);
            return Ok(false);
        }

        print!("Enter password: ");
        io::stdout().flush()?;
        let mut password = String::new();
        io::stdin().read_line(&mut password)?;
        let password = password.trim().to_string();

        if password.is_empty() {
            println!("❌ Password cannot be empty!");
            return Ok(false);
        }

        if password.len() < 6 {
            println!("❌ Password must be at least 6 characters!");
            return Ok(false);
        }

        print!("Confirm password: ");
        io::stdout().flush()?;
        let mut confirm = String::new();
        io::stdin().read_line(&mut confirm)?;
        let confirm = confirm.trim().to_string();

        if password != confirm {
            println!("❌ Passwords don't match!");
            return Ok(false);
        }

        let user = User::new(username.clone(), password);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(USERS_FILE)?;
        writeln!(file, "{}:{}", user.username, user.password_hash)?;

        println!("✅ Registration successful! You can now login.");
        Ok(true)
    }

    pub fn login(&mut self) -> io::Result<bool> {
        println!("\n=== LOGIN ===");
        print!("Enter username: ");
        io::stdout().flush()?;
        let mut username = String::new();
        io::stdin().read_line(&mut username)?;
        let username = username.trim().to_string();

        print!("Enter password: ");
        io::stdout().flush()?;
        let mut password = String::new();
        io::stdin().read_line(&mut password)?;
        let password = password.trim().to_string();

        if let Some(user) = self.load_user(&username)? {
            if user.verify_password(&password) {
                self.current_user = Some(user);
                println!("✅ Login successful! Welcome, {}!", username);
                return Ok(true);
            }
        }

        println!("❌ Invalid username or password!");
        Ok(false)
    }

    pub fn is_logged_in(&self) -> bool {
        self.current_user.is_some()
    }

    pub fn get_current_user(&self) -> Option<&User> {
        self.current_user.as_ref()
    }

    pub fn logout(&mut self) {
        self.current_user = None;
        println!("✅ Logged out successfully!");
    }

    fn user_exists(&self, username: &str) -> io::Result<bool> {
        let file = File::open(USERS_FILE)?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            if let Some(stored_username) = line.split(':').next() {
                if stored_username == username {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn load_user(&self, username: &str) -> io::Result<Option<User>> {
        let file = File::open(USERS_FILE)?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() == 2 {
                let stored_username = parts[0];
                let password_hash = parts[1];
                if stored_username == username {
                    return Ok(Some(User {
                        username: username.to_string(),
                        password_hash: password_hash.to_string(),
                    }));
                }
            }
        }
        Ok(None)
    }
}

pub fn show_welcome_menu() -> io::Result<i32> {
    println!("\n");
    println!("╔════════════════════════════════════════════╗");
    println!("║                                            ║");
    println!("║         WELCOME TO CHESS ENGINE            ║");
    println!("║                                            ║");
    println!("╚════════════════════════════════════════════╝");
    println!();
    println!("┌────────────────────────────────────────────┐");
    println!("│  1. Register                               │");
    println!("│  2. Login                                  │");
    println!("│  3. Exit                                   │");
    println!("└────────────────────────────────────────────┘");
    println!();
    print!("Select option (1-3): ");
    io::stdout().flush()?;

    let mut choice = String::new();
    io::stdin().read_line(&mut choice)?;

    match choice.trim().parse::<i32>() {
        Ok(n) if (1..=3).contains(&n) => Ok(n),
        _ => {
            println!("❌ Invalid choice! Please enter 1, 2, or 3.");
            Ok(0)
        }
    }
}
