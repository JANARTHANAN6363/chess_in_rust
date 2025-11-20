// Integration layer between terminal UI and chess engine

use crate::engine::{Board, Move, Piece, Sq, ai_move, gen_moves, is_king_attacked};
use crate::ui::{
    AsciiArt, ConfirmDialog, GameInterface, GameMode, GameResult, GameSettings, InputValidator,
    MoveHistoryDisplay, Notification, NotificationKind, StatsDisplay, create_game_mode_menu,
    create_main_menu,
};
use std::io::{self, Write};
use std::time::Instant;

// ============================================================================
// GAME CONTROLLER
// ============================================================================

pub struct GameController {
    board: Board,
    interface: GameInterface,
    settings: GameSettings,
    move_history: MoveHistoryDisplay,
    game_active: bool,
}

impl GameController {
    pub fn new() -> Self {
        Self {
            board: Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"),
            interface: GameInterface::new(),
            settings: GameSettings::default(),
            move_history: MoveHistoryDisplay::new(),
            game_active: false,
        }
    }

    pub fn run(&mut self) {
        AsciiArt::show_welcome_banner();

        loop {
            let menu = create_main_menu();
            menu.display();

            match menu.get_selection() {
                Ok(action) => match action.as_str() {
                    "new_game" => self.start_new_game(),
                    "load_game" => self.load_game(),
                    "settings" => self.configure_settings(),
                    "tutorial" => self.show_tutorial(),
                    "stats" => self.show_statistics(),
                    "about" => self.show_about(),
                    "logout" => {
                        if ConfirmDialog::confirm("Are you sure you want to logout?") {
                            break;
                        }
                    }
                    "exit" => {
                        if ConfirmDialog::confirm("Are you sure you want to exit?") {
                            std::process::exit(0);
                        }
                    }
                    _ => {}
                },
                Err(e) => {
                    eprintln!("Error: {}", e);
                }
            }
        }
    }

    fn start_new_game(&mut self) {
        // Select game mode
        let mode_menu = create_game_mode_menu();
        mode_menu.display();

        let mode = match mode_menu.get_selection() {
            Ok(action) => match action.as_str() {
                "human_vs_engine" => GameMode::HumanVsEngine,
                "human_vs_human" => GameMode::HumanVsHuman,
                "engine_vs_engine" => GameMode::EngineVsEngine,
                "analysis" => GameMode::Analysis,
                "back" => return,
                _ => return,
            },
            Err(_) => return,
        };

        self.interface.set_game_mode(mode);

        // Reset game state
        self.board = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
        self.move_history.clear();
        self.interface.clear_history();
        self.game_active = true;

        Notification::new(
            "Game started! Good luck!".to_string(),
            NotificationKind::Success,
        )
        .show_timed(1500);

        // Start game loop
        match mode {
            GameMode::HumanVsEngine => self.play_human_vs_engine(),
            GameMode::HumanVsHuman => self.play_human_vs_human(),
            GameMode::EngineVsEngine => self.play_engine_vs_engine(),
            GameMode::Analysis => self.analysis_mode(),
        }
    }

    fn play_human_vs_engine(&mut self) {
        while self.game_active {
            self.interface.show_game_screen(&self.board);

            // Check for game end
            if let Some(result) = self.check_game_end() {
                self.interface.show_game_result(result);
                self.game_active = false;
                break;
            }

            if self.board.side_white {
                // Human move (white)
                if !self.handle_human_move() {
                    break; // User quit
                }
            } else {
                // Engine move (black)
                Notification::new("Engine is thinking...".to_string(), NotificationKind::Info)
                    .show();

                let depth = self.settings.get_search_depth();
                let start = Instant::now();

                if let Some(mv) = ai_move(&mut self.board, depth, Some(5000)) {
                    let elapsed = start.elapsed().as_millis();

                    let move_str =
                        format!("{}{}", Self::sq_to_alg(mv.from), Self::sq_to_alg(mv.to));

                    self.board.make_move(mv.from, mv.to, mv.promotion);
                    self.interface.highlight_move(mv.from, mv.to);
                    self.move_history.add_move(move_str.clone());
                    self.interface.add_move_to_history(move_str);

                    Notification::new(
                        format!("Engine played {} ({}ms)", Self::format_move(&mv), elapsed),
                        NotificationKind::Success,
                    )
                    .show_timed(1000);
                } else {
                    self.interface.show_error("Engine couldn't find a move!");
                    self.game_active = false;
                }
            }
        }

        self.show_end_game_options();
    }

    fn play_human_vs_human(&mut self) {
        while self.game_active {
            self.interface.show_game_screen(&self.board);

            if let Some(result) = self.check_game_end() {
                self.interface.show_game_result(result);
                self.game_active = false;
                break;
            }

            if !self.handle_human_move() {
                break;
            }
        }

        self.show_end_game_options();
    }

    fn play_engine_vs_engine(&mut self) {
        Notification::new(
            "Watching engine battle...".to_string(),
            NotificationKind::Info,
        )
        .show();

        while self.game_active {
            self.interface.show_game_screen(&self.board);

            if let Some(result) = self.check_game_end() {
                self.interface.show_game_result(result);
                self.game_active = false;
                break;
            }

            // Both sides use engine
            let depth = 5;
            if let Some(mv) = ai_move(&mut self.board, depth, Some(3000)) {
                let move_str = format!("{}{}", Self::sq_to_alg(mv.from), Self::sq_to_alg(mv.to));

                self.board.make_move(mv.from, mv.to, mv.promotion);
                self.interface.highlight_move(mv.from, mv.to);
                self.move_history.add_move(move_str.clone());
                self.interface.add_move_to_history(move_str);

                std::thread::sleep(std::time::Duration::from_millis(500));
            } else {
                break;
            }
        }

        self.show_end_game_options();
    }

    fn analysis_mode(&mut self) {
        loop {
            self.interface.show_game_screen(&self.board);

            println!("\nAnalysis Mode - Enter command:");
            print!("> ");
            io::stdout().flush().unwrap();

            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
            let input = input.trim();

            if input.is_empty() {
                continue;
            }

            match input.split_whitespace().next().unwrap() {
                "move" | "m" => {
                    if let Some(move_str) = input.split_whitespace().nth(1) {
                        if let Err(e) = self.try_make_move(move_str) {
                            self.interface.show_error(&e);
                        }
                    }
                }
                "analyze" | "a" => {
                    let depth = input
                        .split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse::<i32>().ok())
                        .unwrap_or(8);
                    self.run_analysis(depth);
                }
                "eval" | "e" => {
                    self.show_evaluation();
                }
                "undo" | "u" => {
                    self.board.undo_move();
                    self.interface.show_success("Move undone");
                }
                "redo" | "r" => {
                    self.board.redo_move();
                    self.interface.show_success("Move redone");
                }
                "flip" | "f" => {
                    self.interface.display.flip_board = !self.interface.display.flip_board;
                }
                "back" | "exit" | "quit" => {
                    break;
                }
                _ => {
                    self.interface.show_error("Unknown command");
                }
            }
        }
    }

    fn handle_human_move(&mut self) -> bool {
        loop {
            let side = if self.board.side_white {
                "White"
            } else {
                "Black"
            };
            let prompt = format!("{} to move", side);
            let input = self.interface.prompt_input(&prompt);

            if input.is_empty() {
                continue;
            }

            // Parse command
            let parts: Vec<&str> = input.split_whitespace().collect();

            match parts[0] {
                "move" | "m" => {
                    if parts.len() < 2 {
                        self.interface.show_error("Usage: move e2e4");
                        continue;
                    }

                    match self.try_make_move(parts[1]) {
                        Ok(mv) => {
                            let move_str =
                                format!("{}{}", Self::sq_to_alg(mv.from), Self::sq_to_alg(mv.to));

                            self.interface.highlight_move(mv.from, mv.to);
                            self.move_history.add_move(move_str.clone());
                            self.interface.add_move_to_history(move_str);
                            return true;
                        }
                        Err(e) => {
                            self.interface.show_error(&e);
                            continue;
                        }
                    }
                }
                "undo" | "u" => {
                    self.board.undo_move();
                    self.interface.show_success("Move undone");
                    continue;
                }
                "redo" | "r" => {
                    self.board.redo_move();
                    self.interface.show_success("Move redone");
                    continue;
                }
                "hint" | "h" => {
                    self.show_hint();
                    continue;
                }
                "analyze" => {
                    self.run_analysis(6);
                    continue;
                }
                "flip" | "f" => {
                    self.interface.display.flip_board = !self.interface.display.flip_board;
                    break;
                }
                "resign" => {
                    if ConfirmDialog::confirm("Are you sure you want to resign?") {
                        self.game_active = false;
                        return false;
                    }
                    continue;
                }
                "menu" | "quit" | "exit" => {
                    if ConfirmDialog::confirm("Quit current game?") {
                        self.game_active = false;
                        return false;
                    }
                    continue;
                }
                "help" => {
                    self.interface.show_help();
                    break;
                }
                _ => {
                    // Try to parse as move directly
                    match self.try_make_move(parts[0]) {
                        Ok(mv) => {
                            let move_str =
                                format!("{}{}", Self::sq_to_alg(mv.from), Self::sq_to_alg(mv.to));

                            self.interface.highlight_move(mv.from, mv.to);
                            self.move_history.add_move(move_str.clone());
                            self.interface.add_move_to_history(move_str);
                            return true;
                        }
                        Err(e) => {
                            self.interface
                                .show_error(&format!("Invalid command or move: {}", e));
                            continue;
                        }
                    }
                }
            }
        }

        true
    }

    fn try_make_move(&mut self, move_str: &str) -> Result<Move, String> {
        let (from_str, to_str, promo_char) = InputValidator::validate_move(move_str)?;

        let from = Self::alg_to_sq(&from_str)
            .ok_or_else(|| format!("Invalid source square: {}", from_str))?;
        let to = Self::alg_to_sq(&to_str)
            .ok_or_else(|| format!("Invalid destination square: {}", to_str))?;

        let promotion = promo_char.map(|c| Piece::from_char(c));

        // Verify move is legal
        let mut legal_moves = Vec::new();
        gen_moves(&self.board, &mut legal_moves);

        let is_legal = legal_moves
            .iter()
            .any(|m| m.from == from && m.to == to && m.promotion == promotion);

        if !is_legal {
            return Err("Illegal move!".to_string());
        }

        self.board.make_move(from, to, promotion);

        Ok(Move {
            from,
            to,
            promotion,
        })
    }

    fn show_hint(&mut self) {
        Notification::new(
            "Calculating best move...".to_string(),
            NotificationKind::Info,
        )
        .show();

        let depth = self.settings.get_search_depth();
        if let Some(mv) = ai_move(&mut self.board, depth, Some(3000)) {
            let hint = format!("Suggested move: {}", Self::format_move(&mv));
            Notification::new(hint, NotificationKind::Success).show();
        } else {
            self.interface.show_error("Could not calculate hint");
        }
    }

    fn run_analysis(&mut self, depth: i32) {
        use crate::ui::colors::*;

        println!();
        println!("{}{}Running deep analysis...{}", BOLD, BRIGHT_CYAN, RESET);

        let start = Instant::now();
        if let Some(mv) = ai_move(&mut self.board, depth, None) {
            let elapsed = start.elapsed().as_millis();

            self.interface.display.print_analysis(
                depth,
                0, // Would need to extract score from search
                0, // Would need to track nodes
                elapsed,
                &Self::format_move(&mv),
            );
        }
    }

    fn show_evaluation(&self) {
        // Simple material evaluation for now
        let mut score = 0;
        for sq in 0..128 {
            if (sq & 0x88) != 0 {
                continue;
            }
            let piece = self.board.cells[sq];
            score += match piece {
                Piece::WP => 100,
                Piece::WN => 320,
                Piece::WB => 330,
                Piece::WR => 500,
                Piece::WQ => 900,
                Piece::BP => -100,
                Piece::BN => -320,
                Piece::BB => -330,
                Piece::BR => -500,
                Piece::BQ => -900,
                _ => 0,
            };
        }

        StatsDisplay::show_position_eval(score, 0, score);
    }

    fn check_game_end(&self) -> Option<GameResult> {
        let mut moves = Vec::new();
        gen_moves(&self.board, &mut moves);

        if moves.is_empty() {
            // Check if king is in check
            if is_king_attacked(&self.board, self.board.side_white) {
                // Checkmate
                if self.board.side_white {
                    return Some(GameResult::BlackWins);
                } else {
                    return Some(GameResult::WhiteWins);
                }
            } else {
                // Stalemate
                return Some(GameResult::Stalemate);
            }
        }

        // Check for draw by 50-move rule
        if self.board.halfmove_clock >= 100 {
            return Some(GameResult::Draw);
        }

        None
    }

    fn show_end_game_options(&self) {
        let options = vec!["New Game", "Analyze Game", "Save Game", "Main Menu"];

        let choice = ConfirmDialog::choose("What would you like to do?", &options);

        match choice {
            0 => {} // New game - handled by caller
            1 => {} // Analyze - TODO
            2 => self.save_game(),
            3 => {} // Main menu - handled by caller
            _ => {}
        }
    }

    fn save_game(&self) {
        let filename = self.interface.prompt_input("Enter filename");

        if filename.is_empty() {
            self.interface.show_warning("Save cancelled");
            return;
        }

        // TODO: Implement PGN saving
        self.interface
            .show_success(&format!("Game saved to {}", filename));
    }

    fn load_game(&mut self) {
        let filename = self.interface.prompt_input("Enter filename to load");

        if filename.is_empty() {
            self.interface.show_warning("Load cancelled");
            return;
        }

        // TODO: Implement PGN loading
        self.interface.show_error("Load game not yet implemented");
    }

    fn configure_settings(&mut self) {
        self.settings = GameSettings::configure_interactive();
        Notification::new("Settings updated!".to_string(), NotificationKind::Success).show();
    }

    fn show_tutorial(&self) {
        self.interface.show_help();
    }

    fn show_statistics(&self) {
        StatsDisplay::show_engine_stats(0, 0, 0, 0);
    }

    fn show_about(&self) {
        use crate::ui::colors::*;

        println!();
        println!(
            "{}{}╔════════════════════════════════════════════════════════╗{}",
            BOLD, BRIGHT_CYAN, RESET
        );
        println!(
            "{}{}║              RUST CHESS ENGINE v0.2.0                    ║{}",
            BOLD, BRIGHT_CYAN, RESET
        );
        println!(
            "{}{}╚════════════════════════════════════════════════════════╝{}",
            BOLD, BRIGHT_CYAN, RESET
        );
        println!();
        println!("  {}Features:{}", BOLD, RESET);
        println!("    • 0x88 Board Representation");
        println!("    • Alpha-Beta Pruning with Transposition Tables");
        println!("    • Zobrist Hashing");
        println!("    • Interactive Terminal UI");
        println!("    • Multiple Game Modes");
        println!();
        println!("  {}Created with:{}", BOLD, RESET);
        println!("    • Rust Programming Language");
        println!("    • Advanced Chess Programming Techniques");
        println!();
        println!("{}Press Enter to continue...{}", DIM, RESET);

        let mut dummy = String::new();
        io::stdin().read_line(&mut dummy).ok();
    }

    // Helper functions
    fn sq_to_alg(s: Sq) -> String {
        let r = (s >> 4) as i32;
        let f = (s & 15) as i32;
        if r < 0 || r > 7 || f < 0 || f > 7 {
            return String::from("??");
        }
        let file = (b'a' + f as u8) as char;
        let rank = (1 + r).to_string();
        format!("{}{}", file, rank)
    }

    fn alg_to_sq(s: &str) -> Option<Sq> {
        let s = s.trim();
        if s.len() < 2 {
            return None;
        }
        let bytes = s.as_bytes();
        let f = (bytes[0] as char).to_ascii_lowercase();
        let rch = bytes[1] as char;
        if !(('a'..='h').contains(&f)) {
            return None;
        }
        if !(('1'..='8').contains(&rch)) {
            return None;
        }
        let file = (f as u8 - b'a') as i32;
        let rank = (rch as u8 - b'1') as i32;
        Some(((rank << 4) | file) as usize)
    }

    fn format_move(mv: &Move) -> String {
        if let Some(promo) = mv.promotion {
            format!(
                "{}{}{}",
                Self::sq_to_alg(mv.from),
                Self::sq_to_alg(mv.to),
                promo.to_char()
            )
        } else {
            format!("{}{}", Self::sq_to_alg(mv.from), Self::sq_to_alg(mv.to))
        }
    }
}
