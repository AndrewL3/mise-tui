#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Quit,
    ReloadConfig,
    FocusNext,
    FocusPrev,
    FocusDirection(Direction),
    Tick,
    Resize(u16, u16),
    Notify(String),
    EnterInteract,
    ExitInteract,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}
