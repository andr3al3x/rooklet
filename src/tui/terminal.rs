//! Restore terminal modes and mouse capture on every exit path.
use anyhow::{Context, Result};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
};
use ratatui::{DefaultTerminal, Frame};
use std::io;

pub(super) struct TerminalSession {
    terminal: DefaultTerminal,
    active: bool,
}

impl TerminalSession {
    pub(super) fn start() -> Result<Self> {
        let terminal = match ratatui::try_init() {
            Ok(terminal) => terminal,
            Err(error) => {
                // Initialization can fail after entering raw mode or the alternate screen.
                let _ = ratatui::try_restore();
                return Err(error).context("cannot initialize terminal");
            }
        };
        let session = Self {
            terminal,
            active: true,
        };
        execute!(io::stdout(), EnableMouseCapture)
            .context("cannot enable terminal mouse capture")?;
        Ok(session)
    }

    pub(super) fn draw(&mut self, draw: impl FnOnce(&mut Frame<'_>)) -> Result<()> {
        self.terminal.draw(draw)?;
        Ok(())
    }

    pub(super) fn suspend(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        // Attempt every cleanup operation, even if writing a terminal command fails.
        let mouse = execute!(io::stdout(), DisableMouseCapture)
            .context("cannot disable terminal mouse capture");
        let cursor = self
            .terminal
            .show_cursor()
            .context("cannot show terminal cursor");
        let restored = ratatui::try_restore().context("cannot restore terminal");
        self.active = mouse.is_err() || cursor.is_err() || restored.is_err();
        mouse.and(cursor).and(restored)
    }

    pub(super) fn resume(&mut self) -> Result<()> {
        // Keep cleanup armed before any operation can partially change terminal modes.
        self.active = true;
        self.terminal = ratatui::try_init().context("cannot resume terminal")?;
        execute!(io::stdout(), EnableMouseCapture)
            .context("cannot enable terminal mouse capture")?;
        Ok(())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.suspend();
    }
}
