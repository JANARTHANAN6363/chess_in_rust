use rust_chess_engine::auth::{AuthSystem, show_welcome_menu};
use rust_chess_engine::ui::GameController;
use std::io;

fn main() {
    let mut auth = AuthSystem::new();

    // Main menu loop
    loop {
        match show_welcome_menu() {
            Ok(1) => {
                // Register
                if let Err(e) = auth.register() {
                    eprintln!("Registration error: {}", e);
                }
            }
            Ok(2) => {
                // Login
                match auth.login() {
                    Ok(true) => {
                        // Successfully logged in, start chess engine
                        println!("\n🎮 Starting Chess Engine...\n");
                        // Create and run the new UI controller
                        let mut controller = GameController::new();
                        controller.run();

                        auth.logout();
                        // After engine exits, return to main menu
                    }
                    Ok(false) => {
                        // Login failed, stay in menu
                    }
                    Err(e) => {
                        eprintln!("Login error: {}", e);
                    }
                }
            }
            Ok(3) => {
                // Exit
                println!("\n👋 Thank you for playing! Goodbye!\n");
                break;
            }
            Ok(0) => {
                // Invalid choice, loop continues
            }
            Ok(_) => {
                println!("❌ Invalid option!");
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                break;
            }
        }

        // Pause before showing menu again
        if !auth.is_logged_in() {
            println!("\nPress Enter to continue...");
            let mut dummy = String::new();
            io::stdin().read_line(&mut dummy).ok();
        }
    }
}
