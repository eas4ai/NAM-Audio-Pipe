// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Fábio Henrique de Lima Silva (fhl.bsb@gmail.com) All rights reserved.

//! Provides a simple `Colorize` trait for formatting strings with
//! ANSI escape sequences in POSIX terminals.
//!
//! **Design Decision:** We opted to keep this minimal local implementation
//! instead of adding external crates (e.g.: `colored`, `owo-colors`) to keep
//! the dependency graph as lean as possible, since the project's need
//! is basic and restricted to non-critical threads (CLI/Logging).

/// Simple trait for formatting strings with ANSI escape sequences in POSIX terminals.
pub trait Colorize {
    /// Applies bold formatting.
    fn bold(&self) -> String;
    /// Colors the text red.
    fn red(&self) -> String;
    /// Colors the text green.
    fn green(&self) -> String;
    /// Colors the text yellow.
    fn yellow(&self) -> String;
    /// Colors the text blue.
    fn blue(&self) -> String;
    /// Colors the text cyan.
    fn cyan(&self) -> String;
    /// Colors the text bright red.
    fn bright_red(&self) -> String;
    /// Colors the text bright green.
    fn bright_green(&self) -> String;
    /// Colors the text bright blue.
    fn bright_blue(&self) -> String;
    /// Colors the text bright cyan.
    fn bright_cyan(&self) -> String;
}

impl<T: AsRef<str>> Colorize for T {
    fn bold(&self) -> String {
        format!("\x1b[1m{}\x1b[0m", self.as_ref())
    }

    fn red(&self) -> String {
        format!("\x1b[31m{}\x1b[0m", self.as_ref())
    }

    fn green(&self) -> String {
        format!("\x1b[32m{}\x1b[0m", self.as_ref())
    }

    fn yellow(&self) -> String {
        format!("\x1b[33m{}\x1b[0m", self.as_ref())
    }

    fn blue(&self) -> String {
        format!("\x1b[34m{}\x1b[0m", self.as_ref())
    }

    fn cyan(&self) -> String {
        format!("\x1b[36m{}\x1b[0m", self.as_ref())
    }

    fn bright_red(&self) -> String {
        format!("\x1b[91m{}\x1b[0m", self.as_ref())
    }

    fn bright_green(&self) -> String {
        format!("\x1b[92m{}\x1b[0m", self.as_ref())
    }

    fn bright_blue(&self) -> String {
        format!("\x1b[94m{}\x1b[0m", self.as_ref())
    }

    fn bright_cyan(&self) -> String {
        format!("\x1b[96m{}\x1b[0m", self.as_ref())
    }
}
